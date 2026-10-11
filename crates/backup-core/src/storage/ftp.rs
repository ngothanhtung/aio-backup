//! Đích FTP/FTPS.
//!
//! FTP không cho đặt mtime một cách đáng tin cậy (MFMT không phổ biến) và liệt kê
//! cả cây thư mục rất chậm, nên trạng thái ở đích được lưu trong một manifest
//! `.aio-manifest.json` ở thư mục gốc của job: map đường dẫn -> size + mtime
//! của file nguồn tại thời điểm upload. Manifest được ghi lại định kỳ trong lúc
//! chạy để lượt bị ngắt vẫn giữ được phần đã upload.
//!
//! Upload atomic: STOR vào `file.backup-tmp` rồi RNFR/RNTO sang tên thật.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use suppaftp::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use suppaftp::rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use suppaftp::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use suppaftp::rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use suppaftp::types::FileType;
use suppaftp::{FtpError, RustlsConnector, RustlsFtpStream};

use super::{snapshot_older_than, to_slash, Storage};
use crate::config::FtpTarget;
use crate::error::{BackupError, IoContext, Result};
use crate::scan::{FileEntry, Filter, ScanResult};
use crate::{MANIFEST_FILE, TMP_SUFFIX, VERSIONS_DIR};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IO_TIMEOUT: Duration = Duration::from_secs(120);
/// Ghi manifest sau mỗi N file hoặc mỗi khoảng thời gian, cái nào đến trước.
const FLUSH_EVERY_FILES: usize = 50;
const FLUSH_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ManifestEntry {
    size: u64,
    /// mtime của file nguồn lúc upload (giây + nano từ UNIX epoch).
    mtime: i64,
    #[serde(default)]
    mtime_ns: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    format: u32,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    files: BTreeMap<String, ManifestEntry>,
}

pub struct FtpStorage {
    stream: RustlsFtpStream,
    url: String,
    /// Thư mục gốc của job trên server, dạng `/backup/Tai lieu` hoặc `Tai lieu`.
    root: String,
    manifest: Manifest,
    dirty: usize,
    last_flush: Instant,
    made_dirs: HashSet<String>,
}

fn ftp_err(context: &str, e: FtpError) -> BackupError {
    BackupError::Remote(format!("{context}: {e}"))
}

/// Ghép đường dẫn FTP, bỏ `/` thừa.
fn join(a: &str, b: &str) -> String {
    let abs = a.starts_with('/');
    let (a, b) = (a.trim_matches('/'), b.trim_matches('/'));
    let mut s = String::with_capacity(a.len() + b.len() + 2);
    if abs {
        s.push('/');
    }
    s.push_str(a);
    if !a.is_empty() && !b.is_empty() {
        s.push('/');
    }
    s.push_str(b);
    s
}

fn parent(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

impl FtpStorage {
    /// Kết nối, đăng nhập và trỏ tới thư mục `<remote_dir>/<subdir>`.
    pub fn connect(cfg: &FtpTarget, password: &str, subdir: &str) -> Result<Self> {
        let addr = (cfg.host.as_str(), cfg.port)
            .to_socket_addrs()
            .map_err(|e| BackupError::Remote(format!("Không phân giải được {}: {e}", cfg.host)))?
            .next()
            .ok_or_else(|| BackupError::Remote(format!("Không tìm thấy địa chỉ của {}", cfg.host)))?;

        let mut stream = RustlsFtpStream::connect_timeout(addr, CONNECT_TIMEOUT)
            .map_err(|e| ftp_err(&format!("Không kết nối được {}:{}", cfg.host, cfg.port), e))?;
        let _ = stream.get_ref().set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.get_ref().set_write_timeout(Some(IO_TIMEOUT));

        if cfg.secure {
            let connector = RustlsConnector::from(Arc::new(tls_config(cfg.accept_invalid_certs)?));
            stream = stream.into_secure(connector, &cfg.host).map_err(|e| {
                let msg = e.to_string();
                let hint = if msg.contains("certificate") && !cfg.accept_invalid_certs {
                    ". Server dùng chứng chỉ tự ký? Bật \"Chấp nhận chứng chỉ tự ký\" nếu đây là NAS trong mạng nội bộ"
                } else {
                    ""
                };
                BackupError::Remote(format!("Bắt tay TLS (FTPS) thất bại: {msg}{hint}"))
            })?;
        }

        stream
            .login(cfg.username.as_str(), password)
            .map_err(|e| ftp_err("Đăng nhập thất bại", e))?;
        stream
            .transfer_type(FileType::Binary)
            .map_err(|e| ftp_err("TYPE I", e))?;
        // Server sau NAT hay trả IP nội bộ trong PASV -> dùng IP của control connection
        stream.set_passive_nat_workaround(true);

        let root = join(&cfg.remote_dir, subdir);
        let url = join(&cfg.url(), subdir);
        Ok(Self {
            stream,
            url,
            root,
            manifest: Manifest::default(),
            dirty: 0,
            last_flush: Instant::now(),
            made_dirs: HashSet::new(),
        })
    }

    fn remote(&self, rel: &str) -> String {
        join(&self.root, rel)
    }

    /// `mkdir -p`. MKD báo lỗi khi thư mục đã có, nên bỏ qua lỗi ở đây;
    /// lỗi thật (không có quyền) sẽ lộ ra ở lệnh STOR ngay sau đó.
    fn mkdirs(&mut self, dir: &str) {
        if dir.is_empty() || self.made_dirs.contains(dir) {
            return;
        }
        let absolute = dir.starts_with('/');
        let mut cur = String::new();
        for part in dir.split('/').filter(|s| !s.is_empty()) {
            cur = if cur.is_empty() && !absolute {
                part.to_string()
            } else {
                format!("{cur}/{part}")
            };
            if self.made_dirs.insert(cur.clone()) {
                let _ = self.stream.mkdir(&cur);
            }
        }
    }

    fn load_manifest(&mut self) -> Manifest {
        for name in [MANIFEST_FILE.to_string(), format!("{MANIFEST_FILE}{TMP_SUFFIX}")] {
            let path = self.remote(&name);
            if let Ok(buf) = self.stream.retr_as_buffer(&path) {
                if let Ok(m) = serde_json::from_slice::<Manifest>(buf.get_ref()) {
                    return m;
                }
            }
        }
        Manifest::default()
    }

    fn flush_manifest(&mut self) -> Result<()> {
        self.manifest.format = 1;
        self.manifest.updated_at = Some(Utc::now());
        let json = serde_json::to_vec(&self.manifest).map_err(anyhow::Error::from)?;
        let target = self.remote(MANIFEST_FILE);
        let tmp = format!("{target}{TMP_SUFFIX}");
        let root = self.root.clone();
        self.mkdirs(&root);
        self.stream
            .put_file(&tmp, &mut json.as_slice())
            .map_err(|e| ftp_err("Ghi manifest", e))?;
        // Nhiều server (IIS) không cho RNTO đè file có sẵn -> xoá trước.
        // Nếu chết giữa chừng, lần sau load_manifest đọc được bản .backup-tmp.
        let _ = self.stream.rm(&target);
        self.stream
            .rename(&tmp, &target)
            .map_err(|e| ftp_err("Đổi tên manifest", e))?;
        self.dirty = 0;
        self.last_flush = Instant::now();
        Ok(())
    }

    fn maybe_flush(&mut self) {
        if self.dirty >= FLUSH_EVERY_FILES || (self.dirty > 0 && self.last_flush.elapsed() >= FLUSH_EVERY) {
            if let Err(e) = self.flush_manifest() {
                tracing::warn!("flush manifest: {e}");
            }
        }
    }

    /// Xoá đệ quy một thư mục trên server.
    fn rm_tree(&mut self, path: &str) -> Result<()> {
        let lines = self
            .stream
            .list(Some(path))
            .map_err(|e| ftp_err(&format!("LIST {path}"), e))?;
        for line in lines {
            let Ok(f) = suppaftp::list::File::try_from(line.as_str()) else {
                continue;
            };
            let name = f.name();
            if name == "." || name == ".." {
                continue;
            }
            let child = join(path, name);
            if f.is_directory() {
                self.rm_tree(&child)?;
            } else {
                self.stream.rm(&child).map_err(|e| ftp_err(&format!("DELE {child}"), e))?;
            }
        }
        self.stream.rmdir(path).map_err(|e| ftp_err(&format!("RMD {path}"), e))
    }

    /// Dọn thư mục rỗng phía trên `path` (RMD tự thất bại nếu còn file).
    fn rmdir_empty_parents(&mut self, path: &str) {
        let mut dir = parent(path).to_string();
        while !dir.is_empty() && dir != self.root && dir.len() > self.root.len() {
            if self.stream.rmdir(&dir).is_err() {
                break;
            }
            self.made_dirs.remove(&dir);
            dir = parent(&dir).to_string();
        }
    }
}

impl Storage for FtpStorage {
    fn describe(&self) -> String {
        self.url.clone()
    }

    fn prepare(&mut self) -> Result<()> {
        let root = self.root.clone();
        self.mkdirs(&root);
        Ok(())
    }

    fn index(&mut self, filter: &Filter) -> Result<ScanResult> {
        self.manifest = self.load_manifest();
        let mut out = ScanResult::default();
        for (rel, e) in &self.manifest.files {
            let rel = PathBuf::from(rel);
            if filter.is_excluded(&rel) {
                continue;
            }
            let mtime = if e.mtime >= 0 {
                UNIX_EPOCH + Duration::new(e.mtime as u64, e.mtime_ns)
            } else {
                UNIX_EPOCH
            };
            out.files.insert(rel, FileEntry { size: e.size, mtime });
        }
        Ok(out)
    }

    fn local_path(&self, _rel: &Path) -> Option<PathBuf> {
        None
    }

    fn available_space(&mut self) -> Option<u64> {
        None
    }

    fn put(&mut self, src: &Path, rel: &Path, keep_old: Option<&Path>) -> Result<()> {
        let r = to_slash(rel);
        let target = self.remote(&r);
        let tmp = format!("{target}{TMP_SUFFIX}");
        let md = std::fs::metadata(src).at(src)?;

        self.mkdirs(parent(&target));
        let mut reader = BufReader::with_capacity(1 << 20, File::open(src).at(src)?);
        self.stream
            .put_file(&tmp, &mut reader)
            .map_err(|e| ftp_err(&format!("Upload {r}"), e))?;

        let existed = self.manifest.files.contains_key(&r);
        if existed {
            match keep_old {
                Some(keep) => {
                    let keep = self.remote(&to_slash(keep));
                    self.mkdirs(parent(&keep));
                    // File cũ có thể đã bị ai đó xoá tay -> không coi là lỗi
                    let _ = self.stream.rename(&target, &keep);
                }
                None => {
                    let _ = self.stream.rm(&target);
                }
            }
        }
        if let Err(e) = self.stream.rename(&tmp, &target) {
            let _ = self.stream.rm(&tmp);
            return Err(ftp_err(&format!("Đổi tên {r}"), e));
        }

        let (mtime, mtime_ns) = match md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()) {
            Some(d) => (d.as_secs() as i64, d.subsec_nanos()),
            None => (0, 0),
        };
        self.manifest.files.insert(
            r,
            ManifestEntry {
                size: md.len(),
                mtime,
                mtime_ns,
            },
        );
        self.dirty += 1;
        self.maybe_flush();
        Ok(())
    }

    fn remove(&mut self, rel: &Path, keep: Option<&Path>) -> Result<()> {
        let r = to_slash(rel);
        let target = self.remote(&r);
        let res = match keep {
            Some(k) => {
                let k = self.remote(&to_slash(k));
                self.mkdirs(parent(&k));
                self.stream.rename(&target, &k)
            }
            None => self.stream.rm(&target),
        };
        // File đã không còn trên server thì vẫn coi như xoá xong
        if let Err(e) = res {
            if self.stream.size(&target).is_ok() {
                return Err(ftp_err(&format!("Xoá {r}"), e));
            }
        }
        self.manifest.files.remove(&r);
        self.rmdir_empty_parents(&target);
        self.dirty += 1;
        self.maybe_flush();
        Ok(())
    }

    fn is_alive(&mut self) -> bool {
        self.stream.noop().is_ok()
    }

    fn finish(&mut self) -> Result<()> {
        if self.dirty > 0 || self.manifest.updated_at.is_none() {
            self.flush_manifest()?;
        }
        Ok(())
    }

    fn prune_versions(&mut self, keep_days: u32) -> Result<usize> {
        let dir = self.remote(VERSIONS_DIR);
        let Ok(names) = self.stream.nlst(Some(&dir)) else {
            return Ok(0);
        };
        let mut removed = 0;
        for n in names {
            // Có server trả tên đầy đủ, có server chỉ trả tên -> lấy phần cuối
            let name = n.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
            if snapshot_older_than(&name, keep_days) {
                self.rm_tree(&join(&dir, &name))?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

impl Drop for FtpStorage {
    fn drop(&mut self) {
        let _ = self.stream.quit();
    }
}

/// Kiểm tra nhanh: kết nối, đăng nhập, thử ghi và xoá một file nhỏ.
pub fn test_connection(cfg: &FtpTarget, password: &str) -> Result<String> {
    let mut s = FtpStorage::connect(cfg, password, "")?;
    let welcome = s.stream.get_welcome_msg().unwrap_or("").trim().to_string();
    let root = s.root.clone();
    s.mkdirs(&root);
    let probe = s.remote(&format!(".aio-write-test{TMP_SUFFIX}"));
    s.stream
        .put_file(&probe, &mut &b"ok"[..])
        .map_err(|e| ftp_err("Không có quyền ghi vào thư mục đích", e))?;
    let _ = s.stream.rm(&probe);
    Ok(if welcome.is_empty() {
        format!("Kết nối và ghi thử thành công: {}", cfg.url())
    } else {
        format!("Kết nối và ghi thử thành công ({welcome})")
    })
}

fn tls_config(accept_invalid: bool) -> Result<ClientConfig> {
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| BackupError::Remote(format!("TLS: {e}")))?;
    let cfg = if accept_invalid {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
            .with_no_client_auth()
    } else {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Ok(cfg)
}

/// Chấp nhận mọi chứng chỉ (NAS dùng chứng chỉ tự ký trong LAN).
/// Chữ ký handshake vẫn được kiểm tra, chỉ bỏ qua chuỗi tin cậy và tên miền.
#[derive(Debug)]
struct AcceptAnyCert(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, suppaftp::rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, suppaftp::rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, suppaftp::rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_join() {
        assert_eq!(join("/backup", "Tai lieu"), "/backup/Tai lieu");
        assert_eq!(join("/backup/", "/a/"), "/backup/a");
        assert_eq!(join("", "a/b.txt"), "a/b.txt");
        assert_eq!(join("/", "x"), "/x");
        assert_eq!(join("/", ""), "/");
        assert_eq!(parent("/backup/a/b.txt"), "/backup/a");
        assert_eq!(parent("b.txt"), "");
    }
}
