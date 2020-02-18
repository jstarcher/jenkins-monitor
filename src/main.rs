extern crate job_scheduler;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

mod config;

use self::config::Config;
use self::config::ConfigReader;
use chrono::prelude::*;
use jenkins_api::JenkinsBuilder;
use jenkins_api::build::BuildStatus;
use job_scheduler::{JobScheduler, Job};
use std::time::Duration;

lazy_static! {
    static ref APP_CONF: Config = ConfigReader::make();
}

fn check_job(job_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let jenkins = JenkinsBuilder::new(&APP_CONF.jenkins.url)
        .with_user(&APP_CONF.jenkins.username, Some(&APP_CONF.jenkins.password))
        .build()?;

    let job = jenkins.get_job(job_name)?;

    if let Some(short_build) = job.last_build.clone() {
        let build = short_build.get_full_build(&jenkins).unwrap();
        println!(
            "last build for job {} at {} was {:?}",
            job.name, build.timestamp, build.result
        );
        let now_milli = now.timestamp_millis() as i64;
        let timestamp = build.timestamp as i64;
        let last_ran = ((now_milli - timestamp) / 1000) / 60;
        println!("Job last ran {:.2} minutes ago", last_ran);
        if let Some(result) = build.result {
            result != BuildStatus::Success
        } else {
            true
        }
    } else {
        println!("job {} was never built", job.name);
        true
    };
    Ok(())
}

fn main() {
    let mut sched = JobScheduler::new();
    
    for job in &APP_CONF.job {
        println!("Scheduling check for job {:#?}", job.name);
        sched.add(Job::new(job.schedule.parse().unwrap(), || {
            let c = check_job(&job.name.as_ref().unwrap());
            let _c = match c {
                Ok(_) => println!("Success!"),
                Err(e) => println!("Failure: checking job with error {:?}", e)
            };
        }));
    }

    loop {
        sched.tick();

        std::thread::sleep(Duration::from_millis(500));
    }
}
