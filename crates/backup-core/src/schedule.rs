//! Quyết định job nào tới lượt chạy. Dùng chung cho `aio-backup watch` và app desktop.
//!
//! - USB + `auto_run`: chạy khi ổ đích vừa được cắm vào (kể cả lúc app mới mở).
//! - `interval_minutes`: chạy định kỳ; với USB chỉ khi ổ đang cắm.
//! - `schedule`: chạy vào giờ cố định hằng ngày/tuần/tháng. Lỡ giờ (máy tắt, ổ chưa cắm)
//!   thì chạy bù ngay khi có thể.

use std::collections::{HashMap, HashSet};
use std::fmt;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::config::{Destination, Job, Schedule};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// Ổ USB đích vừa được cắm.
    Plugged,
    /// Tới lịch (định kỳ N phút hoặc giờ cố định).
    Interval,
}

impl Schedule {
    fn at(&self) -> NaiveTime {
        match self {
            Schedule::Daily { at } | Schedule::Weekly { at, .. } | Schedule::Monthly { at, .. } => *at,
        }
    }

    /// Lịch có hẹn vào ngày `date` không.
    fn on(&self, date: NaiveDate) -> bool {
        match self {
            Schedule::Daily { .. } => true,
            Schedule::Weekly { weekdays, .. } => weekdays.contains(&(date.weekday().number_from_monday() as u8)),
            Schedule::Monthly { day, .. } => {
                let day = *day as u32;
                let last_of_month = date.succ_opt().is_none_or(|n| n.month() != date.month());
                date.day() == day || (last_of_month && day > date.day())
            }
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Schedule::Daily { .. } => true,
            Schedule::Weekly { weekdays, .. } => {
                !weekdays.is_empty() && weekdays.iter().all(|d| (1..=7).contains(d))
            }
            Schedule::Monthly { day, .. } => (1..=31).contains(day),
        }
    }

    /// Lần hẹn gần nhất không muộn hơn `now` (giờ địa phương).
    pub fn prev(&self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        (0..=366)
            .filter_map(|back| now.date().checked_sub_days(chrono::Days::new(back)))
            .filter(|d| self.on(*d))
            .map(|d| d.and_time(self.at()))
            .find(|t| *t <= now)
    }

    /// Lần hẹn kế tiếp sau `now` (giờ địa phương).
    pub fn next(&self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        (0..=366)
            .filter_map(|ahead| now.date().checked_add_days(chrono::Days::new(ahead)))
            .filter(|d| self.on(*d))
            .map(|d| d.and_time(self.at()))
            .find(|t| *t > now)
    }
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DAYS: [&str; 7] = ["T2", "T3", "T4", "T5", "T6", "T7", "CN"];
        let at = self.at().format("%H:%M");
        match self {
            Schedule::Daily { .. } => write!(f, "hằng ngày lúc {at}"),
            Schedule::Weekly { weekdays, .. } => {
                let days: Vec<&str> = (1..=7u8)
                    .filter(|d| weekdays.contains(d))
                    .map(|d| DAYS[d as usize - 1])
                    .collect();
                write!(f, "{} hằng tuần lúc {at}", days.join(", "))
            }
            Schedule::Monthly { day, .. } => write!(f, "ngày {day} hằng tháng lúc {at}"),
        }
    }
}

fn local(t: DateTime<Utc>) -> NaiveDateTime {
    t.with_timezone(&Local).naive_local()
}

fn utc(t: NaiveDateTime) -> Option<DateTime<Utc>> {
    Local
        .from_local_datetime(&t)
        .earliest()
        // Giờ hẹn rơi vào khoảng bị nhảy qua khi đổi giờ mùa hè -> lùi sang giờ kế tiếp
        .or_else(|| Local.from_local_datetime(&(t + Duration::hours(1))).earliest())
        .map(|d| d.with_timezone(&Utc))
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
    /// Mốc tính lịch giờ cố định cho job chưa chạy lần nào hoặc vừa đổi lịch,
    /// để không chạy bù lần hẹn đã qua từ trước khi lịch được đặt.
    anchor: HashMap<Uuid, DateTime<Utc>>,
}

impl Scheduler {
    /// `last_run` lấy từ lịch sử để mở lại app không chạy lại ngay job vừa chạy.
    pub fn new(last_run: HashMap<Uuid, DateTime<Utc>>) -> Self {
        Self {
            last_run,
            ..Self::default()
        }
    }

    /// Gọi khi lịch giờ cố định của job vừa được đặt/sửa: chỉ tính các lần hẹn sau `at`.
    pub fn reschedule(&mut self, job_id: Uuid, at: DateTime<Utc>) {
        self.anchor.insert(job_id, at);
    }

    /// Ghi nhận một lượt chạy thủ công để lịch định kỳ tính lại từ đây.
    pub fn record_run(&mut self, job_id: Uuid, at: DateTime<Utc>) {
        self.last_run.insert(job_id, at);
    }

    pub fn last_run(&self, job_id: Uuid) -> Option<DateTime<Utc>> {
        self.last_run.get(&job_id).copied()
    }

    /// Lần chạy định kỳ kế tiếp; `None` nếu job không có lịch.
    /// Với USB, thời điểm này chỉ có hiệu lực khi ổ đang cắm.
    pub fn next_run(&self, job: &Job, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let interval = job
            .interval_minutes
            .filter(|m| *m > 0)
            .map(|mins| match self.last_run.get(&job.id) {
                Some(t) => (*t + Duration::minutes(mins as i64)).max(now),
                None => now,
            });
        let fixed = job.schedule.as_ref().and_then(|s| {
            if self.schedule_due(job, now) {
                Some(now)
            } else {
                s.next(local(now)).and_then(utc)
            }
        });
        match (interval, fixed) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Có lần hẹn giờ cố định nào đã tới mà chưa chạy không.
    fn schedule_due(&self, job: &Job, now: DateTime<Utc>) -> bool {
        let Some(schedule) = &job.schedule else {
            return false;
        };
        let since = self.last_run.get(&job.id).max(self.anchor.get(&job.id));
        let Some(since) = since else {
            return false;
        };
        schedule
            .prev(local(now))
            .and_then(utc)
            .is_some_and(|t| t > *since)
    }

    fn due(&self, job: &Job, now: DateTime<Utc>) -> bool {
        self.interval_due(job, now) || self.schedule_due(job, now)
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
            if job.schedule.is_some() && !self.last_run.contains_key(&job.id) {
                self.anchor.entry(job.id).or_insert(now);
            }
            let trigger = match &job.destination {
                Destination::Usb { target_id } => {
                    if !present.contains(target_id) {
                        None
                    } else if job.auto_run && !self.present.contains(target_id) {
                        Some(Trigger::Plugged)
                    } else if self.due(job, now) {
                        Some(Trigger::Interval)
                    } else {
                        None
                    }
                }
                Destination::Ftp(_) => self.due(job, now).then_some(Trigger::Interval),
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
    fn next_run_time() {
        let job = ftp_job(60);
        let t0 = Utc::now();
        let s = Scheduler::new([(job.id, t0 - Duration::minutes(15))].into());
        assert_eq!(s.next_run(&job, t0), Some(t0 + Duration::minutes(45)));
        let s = Scheduler::default();
        assert_eq!(s.next_run(&job, t0), Some(t0));
    }

    fn at(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_time(at(h, mi))
    }

    #[test]
    fn daily_occurrences() {
        let s = Schedule::Daily { at: at(2, 30) };
        assert_eq!(s.next(dt(2026, 1, 15, 1, 0)), Some(dt(2026, 1, 15, 2, 30)));
        assert_eq!(s.next(dt(2026, 1, 15, 2, 30)), Some(dt(2026, 1, 16, 2, 30)));
        assert_eq!(s.prev(dt(2026, 1, 15, 2, 30)), Some(dt(2026, 1, 15, 2, 30)));
        assert_eq!(s.prev(dt(2026, 1, 15, 1, 0)), Some(dt(2026, 1, 14, 2, 30)));
    }

    #[test]
    fn weekly_occurrences() {
        // 15/01/2026 là thứ Năm
        let s = Schedule::Weekly {
            at: at(8, 0),
            weekdays: vec![1, 5],
        };
        assert_eq!(s.next(dt(2026, 1, 15, 9, 0)), Some(dt(2026, 1, 16, 8, 0)));
        assert_eq!(s.next(dt(2026, 1, 16, 9, 0)), Some(dt(2026, 1, 19, 8, 0)));
        assert_eq!(s.prev(dt(2026, 1, 15, 9, 0)), Some(dt(2026, 1, 12, 8, 0)));
        assert!(s.is_valid());

        let empty = Schedule::Weekly {
            at: at(8, 0),
            weekdays: vec![],
        };
        assert!(!empty.is_valid());
        assert_eq!(empty.next(dt(2026, 1, 15, 9, 0)), None);
    }

    #[test]
    fn monthly_falls_back_to_last_day() {
        let s = Schedule::Monthly { at: at(0, 0), day: 31 };
        assert_eq!(s.next(dt(2026, 1, 31, 0, 0)), Some(dt(2026, 2, 28, 0, 0)));
        assert_eq!(s.next(dt(2026, 2, 28, 0, 0)), Some(dt(2026, 3, 31, 0, 0)));
        assert_eq!(s.prev(dt(2026, 4, 29, 0, 0)), Some(dt(2026, 3, 31, 0, 0)));

        let s = Schedule::Monthly { at: at(6, 0), day: 15 };
        assert_eq!(s.next(dt(2026, 1, 15, 6, 0)), Some(dt(2026, 2, 15, 6, 0)));
    }

    #[test]
    fn fixed_time_runs_once_per_occurrence() {
        let mut job = ftp_job(60);
        job.interval_minutes = None;
        job.schedule = Some(Schedule::Daily { at: at(12, 0) });
        let jobs = std::slice::from_ref(&job);
        let none = HashSet::new();
        let t0 = utc(dt(2026, 1, 15, 10, 0)).unwrap();

        // Job mới chưa chạy lần nào: không chạy bù lần hẹn hôm qua
        let mut s = Scheduler::default();
        assert!(s.tick(jobs, &none, t0).is_empty());
        assert_eq!(s.next_run(&job, t0), Some(t0 + Duration::hours(2)));
        assert!(s.tick(jobs, &none, t0 + Duration::hours(1)).is_empty());
        assert_eq!(s.tick(jobs, &none, t0 + Duration::minutes(121)).len(), 1);
        assert!(s.tick(jobs, &none, t0 + Duration::hours(3)).is_empty());
        assert_eq!(
            s.next_run(&job, t0 + Duration::hours(3)),
            Some(t0 + Duration::hours(26))
        );
    }

    #[test]
    fn fixed_time_catches_up_after_downtime() {
        let mut job = ftp_job(60);
        job.interval_minutes = None;
        job.schedule = Some(Schedule::Daily { at: at(12, 0) });
        let jobs = std::slice::from_ref(&job);
        let none = HashSet::new();
        let t0 = utc(dt(2026, 1, 15, 10, 0)).unwrap();

        // Lần chạy cuối cách đây 2 ngày, lỡ lần hẹn 12:00 hôm qua -> chạy bù
        let mut s = Scheduler::new([(job.id, t0 - Duration::days(2))].into());
        assert_eq!(s.next_run(&job, t0), Some(t0));
        assert_eq!(s.tick(jobs, &none, t0).len(), 1);
        assert!(s.tick(jobs, &none, t0 + Duration::hours(1)).is_empty());

        // Vừa đổi lịch thì không chạy bù lần hẹn trước thời điểm đổi
        let mut s = Scheduler::new([(job.id, t0 - Duration::days(2))].into());
        s.reschedule(job.id, t0);
        assert!(s.tick(jobs, &none, t0).is_empty());
        assert_eq!(s.tick(jobs, &none, t0 + Duration::minutes(121)).len(), 1);
    }

    #[test]
    fn ftp_without_interval_never_auto_runs() {
        let mut job = ftp_job(60);
        job.interval_minutes = None;
        let mut s = Scheduler::default();
        assert!(s.tick(&[job], &HashSet::new(), Utc::now()).is_empty());
    }
}
