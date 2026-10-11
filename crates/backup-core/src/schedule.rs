//! Quyết định job nào tới lượt chạy. Dùng chung cho `aio-backup watch` và app desktop.
//!
//! - USB + `auto_run`: chạy khi ổ đích vừa được cắm vào (kể cả lúc app mới mở).
//! - `interval_minutes`: chạy định kỳ; với USB chỉ khi ổ đang cắm.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::config::{Destination, Job};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// Ổ USB đích vừa được cắm.
    Plugged,
    /// Tới lịch định kỳ.
    Interval,
}

#[derive(Debug, Clone)]
pub struct Due {
    pub job: Job,
    pub trigger: Trigger,
}

#[derive(Debug, Default)]
pub struct Scheduler {
    /// Lần chạy (hoặc thử chạy) gần nhất của từng job.
    last_run: HashMap<Uuid, DateTime<Utc>>,
    /// Các ổ đích đang cắm ở lượt tick trước.
    present: HashSet<Uuid>,
}

impl Scheduler {
    /// `last_run` lấy từ lịch sử để mở lại app không chạy lại ngay job vừa chạy.
    pub fn new(last_run: HashMap<Uuid, DateTime<Utc>>) -> Self {
        Self {
            last_run,
            present: HashSet::new(),
        }
    }

    /// Ghi nhận một lượt chạy thủ công để lịch định kỳ tính lại từ đây.
    pub fn record_run(&mut self, job_id: Uuid, at: DateTime<Utc>) {
        self.last_run.insert(job_id, at);
    }

    fn interval_due(&self, job: &Job, now: DateTime<Utc>) -> bool {
        let Some(mins) = job.interval_minutes.filter(|m| *m > 0) else {
            return false;
        };
        match self.last_run.get(&job.id) {
            Some(t) => now - *t >= Duration::minutes(mins as i64),
            None => true,
        }
    }

    /// Gọi định kỳ (vài giây một lần) với danh sách ổ đích đang cắm.
    /// Job được trả về coi như đã bắt đầu chạy tại `now`.
    pub fn tick(&mut self, jobs: &[Job], present: &HashSet<Uuid>, now: DateTime<Utc>) -> Vec<Due> {
        let mut due = vec![];
        for job in jobs {
            let trigger = match &job.destination {
                Destination::Usb { target_id } => {
                    if !present.contains(target_id) {
                        None
                    } else if job.auto_run && !self.present.contains(target_id) {
                        Some(Trigger::Plugged)
                    } else if self.interval_due(job, now) {
                        Some(Trigger::Interval)
                    } else {
                        None
                    }
                }
                Destination::Ftp(_) => self.interval_due(job, now).then_some(Trigger::Interval),
            };
            if let Some(trigger) = trigger {
                self.last_run.insert(job.id, now);
                due.push(Due {
                    job: job.clone(),
                    trigger,
                });
            }
        }
        self.present = present.clone();
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FtpTarget;

    fn ftp_job(every: u32) -> Job {
        let mut j = Job::new(
            "nas",
            "/src",
            Destination::Ftp(FtpTarget {
                host: "nas".into(),
                port: 21,
                username: "u".into(),
                remote_dir: String::new(),
                secure: false,
                accept_invalid_certs: false,
                password: None,
            }),
        );
        j.interval_minutes = Some(every);
        j
    }

    #[test]
    fn usb_runs_on_plug_only_once() {
        let usb = Uuid::new_v4();
        let job = Job::usb("a", "/src", usb);
        let mut s = Scheduler::default();
        let t0 = Utc::now();
        let none = HashSet::new();
        let plugged: HashSet<_> = [usb].into();

        assert!(s.tick(std::slice::from_ref(&job), &none, t0).is_empty());
        let d = s.tick(std::slice::from_ref(&job), &plugged, t0);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].trigger, Trigger::Plugged);
        // Vẫn cắm -> không chạy lại
        assert!(s.tick(std::slice::from_ref(&job), &plugged, t0 + Duration::hours(5)).is_empty());
        // Rút ra cắm lại -> chạy
        s.tick(std::slice::from_ref(&job), &none, t0);
        assert_eq!(s.tick(&[job], &plugged, t0).len(), 1);
    }

    #[test]
    fn ftp_interval() {
        let job = ftp_job(60);
        let t0 = Utc::now();
        let mut s = Scheduler::new([(job.id, t0 - Duration::minutes(30))].into());
        let none = HashSet::new();
        assert!(s.tick(std::slice::from_ref(&job), &none, t0).is_empty());
        let d = s.tick(std::slice::from_ref(&job), &none, t0 + Duration::minutes(31));
        assert_eq!(d[0].trigger, Trigger::Interval);
        assert!(s.tick(std::slice::from_ref(&job), &none, t0 + Duration::minutes(40)).is_empty());
        assert_eq!(s.tick(&[job], &none, t0 + Duration::minutes(92)).len(), 1);
    }

    #[test]
    fn ftp_without_interval_never_auto_runs() {
        let mut job = ftp_job(60);
        job.interval_minutes = None;
        let mut s = Scheduler::default();
        assert!(s.tick(&[job], &HashSet::new(), Utc::now()).is_empty());
    }
}
