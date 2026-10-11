//! Test tích hợp với FTP server thật. Bỏ qua nếu không đặt biến môi trường:
//!
//! ```text
//! AIO_TEST_FTP=127.0.0.1:2121:user:pass      # host:port:user:password
//! AIO_TEST_FTP_ROOT=/srv/ftp/user           # thư mục server phục vụ (để kiểm tra file)
//! AIO_TEST_FTPS=1                           # tuỳ chọn: chạy qua FTPS (chứng chỉ tự ký)
//! ```
//!
//! Server nhanh để thử: `python3 scripts/ftp_test_server.py`

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use backup_core::config::{Destination, FtpTarget, Job};
use backup_core::job::run_job_on;
use backup_core::storage::{FtpStorage, Storage};

struct Env {
    target: FtpTarget,
    password: String,
    root: PathBuf,
}

fn env() -> Option<Env> {
    let spec = std::env::var("AIO_TEST_FTP").ok()?;
    let root = PathBuf::from(std::env::var("AIO_TEST_FTP_ROOT").ok()?);
    let mut it = spec.splitn(4, ':');
    let (host, port, user, pass) = (it.next()?, it.next()?, it.next()?, it.next()?);
    let secure = std::env::var("AIO_TEST_FTPS").is_ok();
    Some(Env {
        target: FtpTarget {
            host: host.into(),
            port: port.parse().ok()?,
            username: user.into(),
            remote_dir: format!("/it-{}", uuid::Uuid::new_v4().simple()),
            secure,
            accept_invalid_certs: secure,
            password: None,
        },
        password: pass.into(),
        root,
    })
}

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

fn run(job: &Job, e: &Env, dry: bool) -> backup_core::engine::Report {
    let mut st = FtpStorage::connect(&e.target, &e.password, "Tai lieu").unwrap();
    let cancel = AtomicBool::new(false);
    run_job_on(job, &mut st, dry, &mut |_| {}, &cancel).unwrap()
}

#[test]
fn ftp_full_cycle() {
    let Some(e) = env() else {
        eprintln!("bỏ qua: chưa đặt AIO_TEST_FTP / AIO_TEST_FTP_ROOT");
        return;
    };
    let src = tempfile::tempdir().unwrap();
    write(&src.path().join("a.txt"), "v1");
    write(&src.path().join("Bài giảng/b c.txt"), "bbb");
    write(&src.path().join("skip.tmp"), "x");

    let mut job = Job::new("Tai lieu", src.path(), Destination::Ftp(e.target.clone()));
    job.excludes = vec!["*.tmp".into()];
    job.mirror_deletes = true;
    let remote = e.root.join(e.target.remote_dir.trim_start_matches('/')).join("Tai lieu");

    // Dry-run không ghi gì
    let r = run(&job, &e, true);
    assert_eq!(r.created, 2);
    assert!(!remote.join("a.txt").exists());

    // Lần 1
    let r = run(&job, &e, false);
    assert!(r.ok(), "{r:?}");
    assert_eq!((r.created, r.updated, r.deleted), (2, 0, 0));
    assert_eq!(fs::read_to_string(remote.join("Bài giảng/b c.txt")).unwrap(), "bbb");
    assert!(remote.join(".aio-manifest.json").exists());
    assert!(!remote.join("skip.tmp").exists());

    // Lần 2: manifest cho biết không có gì đổi
    let r = run(&job, &e, false);
    assert_eq!((r.created, r.updated, r.deleted, r.unchanged), (0, 0, 0, 2));

    // Sửa + xoá -> versioning
    write(&src.path().join("a.txt"), "version 2");
    fs::remove_file(src.path().join("Bài giảng/b c.txt")).unwrap();
    let r = run(&job, &e, false);
    assert!(r.ok(), "{r:?}");
    assert_eq!((r.updated, r.deleted), (1, 1));
    assert_eq!(fs::read_to_string(remote.join("a.txt")).unwrap(), "version 2");
    assert!(!remote.join("Bài giảng").exists(), "thư mục rỗng phải được dọn");
    let snaps: Vec<_> = fs::read_dir(remote.join(".versions")).unwrap().flatten().collect();
    assert_eq!(snaps.len(), 1);
    assert_eq!(fs::read_to_string(snaps[0].path().join("a.txt")).unwrap(), "v1");
    assert_eq!(fs::read_to_string(snaps[0].path().join("Bài giảng/b c.txt")).unwrap(), "bbb");

    // Không còn file tạm
    let leftovers: Vec<_> = walk(&remote).into_iter().filter(|p| p.ends_with(".backup-tmp")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    // Khôi phục qua FTP: bản hiện tại + snapshot
    {
        use backup_core::restore::{list_entries, list_snapshots, restore, RestoreOptions};
        let out = tempfile::tempdir().unwrap();
        let mut st = FtpStorage::connect(&e.target, &e.password, "Tai lieu").unwrap();
        let snaps = list_snapshots(&mut st).unwrap();
        assert_eq!(snaps.len(), 1);
        let cur = list_entries(&mut st, None).unwrap();
        assert_eq!(cur.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), vec!["a.txt"]);
        let old = list_entries(&mut st, Some(&snaps[0].name)).unwrap();
        let mut old_paths: Vec<_> = old.iter().map(|x| x.path.clone()).collect();
        old_paths.sort();
        assert_eq!(old_paths, vec!["Bài giảng/b c.txt".to_string(), "a.txt".to_string()]);
        let cancel = AtomicBool::new(false);
        let opts = RestoreOptions { target: out.path().to_path_buf(), overwrite: false };
        let r = restore(&mut st, Some(&snaps[0].name), &old_paths, &opts, &mut |_, _, _| {}, &cancel).unwrap();
        assert_eq!(r.restored, 2, "{r:?}");
        assert_eq!(fs::read_to_string(out.path().join("Bài giảng/b c.txt")).unwrap(), "bbb");
        assert_eq!(fs::read_to_string(out.path().join("a.txt")).unwrap(), "v1");
    }

    // Dọn snapshot: đổi tên snapshot thành ngày cũ rồi prune
    let old = remote.join(".versions/2000-01-01_000000");
    fs::rename(snaps[0].path(), &old).unwrap();
    let mut st = FtpStorage::connect(&e.target, &e.password, "Tai lieu").unwrap();
    assert_eq!(st.prune_versions(30).unwrap(), 1);
    assert!(!old.exists());

    // Kiểm tra kết nối
    let msg = backup_core::storage::ftp::test_connection(&e.target, &e.password).unwrap();
    assert!(msg.contains("thành công"));
    // Sai mật khẩu -> lỗi rõ ràng
    let err = FtpStorage::connect(&e.target, "wrong", "").err().unwrap().to_string();
    assert!(err.contains("Đăng nhập thất bại"), "{err}");

    fs::remove_dir_all(e.root.join(e.target.remote_dir.trim_start_matches('/'))).ok();
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
    out
}
