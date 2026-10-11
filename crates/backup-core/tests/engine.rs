use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use backup_core::config::Job;
use backup_core::engine::Progress;
use backup_core::storage::{LocalStorage, Storage};
use backup_core::job::{dest_dir, run_job_at};
use backup_core::marker::init_target;
use backup_core::plan::{build_plan, ActionKind, PlanOptions};
use filetime::FileTime;

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

fn job(src: &Path, target: &Path) -> Job {
    let m = init_target(target, "test usb", false).unwrap();
    let mut j = Job::usb("Tai lieu", src, m.id);
    j.excludes = vec!["*.tmp".into()];
    j
}

fn run(j: &Job, target: &Path, dry: bool) -> backup_core::engine::Report {
    let cancel = AtomicBool::new(false);
    run_job_at(j, target, dry, &mut |_| {}, &cancel).unwrap()
}

#[test]
fn full_cycle_with_versioning() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    write(&src.path().join("a.txt"), "v1");
    write(&src.path().join("sub/b.txt"), "bbb");
    write(&src.path().join("sub/skip.tmp"), "x");

    let mut j = job(src.path(), usb.path());
    j.mirror_deletes = true;
    let dst = dest_dir(&j, usb.path());

    // Lần 1: tạo mới
    let r = run(&j, usb.path(), false);
    assert!(r.ok(), "{r:?}");
    assert_eq!((r.created, r.updated, r.deleted), (2, 0, 0));
    assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "bbb");
    assert!(!dst.join("sub/skip.tmp").exists());

    // mtime được giữ nguyên
    let ms = FileTime::from_last_modification_time(&fs::metadata(src.path().join("a.txt")).unwrap());
    let md = FileTime::from_last_modification_time(&fs::metadata(dst.join("a.txt")).unwrap());
    assert_eq!(ms.unix_seconds(), md.unix_seconds());

    // Lần 2: không đổi gì
    let r = run(&j, usb.path(), false);
    assert_eq!((r.created, r.updated, r.deleted, r.unchanged), (0, 0, 0, 2));

    // Sửa a.txt, xoá sub/b.txt
    write(&src.path().join("a.txt"), "version 2");
    fs::remove_file(src.path().join("sub/b.txt")).unwrap();
    let r = run(&j, usb.path(), false);
    assert_eq!((r.updated, r.deleted), (1, 1));
    assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "version 2");
    assert!(!dst.join("sub").exists(), "thư mục rỗng phải được dọn");

    // Bản cũ nằm trong .versions/<timestamp>/
    let snaps: Vec<_> = fs::read_dir(dst.join(".versions")).unwrap().flatten().collect();
    assert_eq!(snaps.len(), 1);
    let snap = snaps[0].path();
    assert_eq!(fs::read_to_string(snap.join("a.txt")).unwrap(), "v1");
    assert_eq!(fs::read_to_string(snap.join("sub/b.txt")).unwrap(), "bbb");

    // Lần sau .versions không bị coi là file cần xoá
    let r = run(&j, usb.path(), false);
    assert_eq!((r.created, r.updated, r.deleted), (0, 0, 0));

    // prune với 0 ngày giữ lại -> chưa xoá vì snapshot vừa tạo (cutoff = now)
    assert_eq!(LocalStorage::new(&dst).prune_versions(1).unwrap(), 0);
}

#[test]
fn dry_run_writes_nothing() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    write(&src.path().join("a.txt"), "x");
    let j = job(src.path(), usb.path());

    let r = run(&j, usb.path(), true);
    assert!(r.dry_run);
    assert_eq!(r.created, 1);
    assert!(!dest_dir(&j, usb.path()).exists());
}

#[test]
fn no_delete_without_mirror() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    write(&src.path().join("a.txt"), "x");
    let j = job(src.path(), usb.path());
    run(&j, usb.path(), false);
    fs::remove_file(src.path().join("a.txt")).unwrap();

    let r = run(&j, usb.path(), false);
    assert_eq!(r.deleted, 0);
    assert!(dest_dir(&j, usb.path()).join("a.txt").exists());
}

#[test]
fn update_without_versioning_overwrites() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    write(&src.path().join("a.txt"), "1");
    let mut j = job(src.path(), usb.path());
    j.versioning = false;
    run(&j, usb.path(), false);
    write(&src.path().join("a.txt"), "22");
    let r = run(&j, usb.path(), false);
    assert_eq!(r.updated, 1);
    assert!(!dest_dir(&j, usb.path()).join(".versions").exists());
}

#[test]
fn cancel_stops_early_and_cleans_tmp() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    for i in 0..20 {
        write(&src.path().join(format!("f{i:02}.txt")), "data");
    }
    let j = job(src.path(), usb.path());
    let dst = dest_dir(&j, usb.path());
    // File tạm sót lại từ lần chạy trước
    write(&dst.join("old.txt.backup-tmp"), "junk");

    let cancel = AtomicBool::new(false);
    let mut seen = 0;
    let r = run_job_at(
        &j,
        usb.path(),
        false,
        &mut |p| {
            if let Progress::File { .. } = p {
                seen += 1;
                if seen == 5 {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        },
        &cancel,
    )
    .unwrap();
    assert!(r.cancelled);
    assert_eq!(r.created, 5);
    assert!(!dst.join("old.txt.backup-tmp").exists());
}

#[test]
fn hash_mode_skips_touched_files() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    write(&src.path().join("a.txt"), "same");
    write(&dst.path().join("a.txt"), "same");
    filetime::set_file_mtime(src.path().join("a.txt"), FileTime::from_unix_time(1_000_000_000, 0)).unwrap();

    let p = build_plan(src.path(), dst.path(), &PlanOptions::default()).unwrap();
    assert_eq!(p.count(ActionKind::Update), 1);

    let opts = PlanOptions { verify_hash: true, ..Default::default() };
    let p = build_plan(src.path(), dst.path(), &opts).unwrap();
    assert!(p.is_empty());
}
