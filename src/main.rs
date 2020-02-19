extern crate cron;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

mod config;

use self::config::Config;
use self::config::ConfigReader;
use cron::Schedule;
use chrono::prelude::*;
use jenkins_api::JenkinsBuilder;
use jenkins_api::build::BuildStatus;
use std::time::Duration;

lazy_static! {
    static ref APP_CONF: Config = ConfigReader::make();
}

fn check_job<'a>(job_name: &str) -> Result<(), Box<dyn std::error::Error>> {
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

fn find_next_run() -> (String, u64) {
    //    parse cron for each job
    //    return minutes until next run
    return (String::from("test1"), 10);
}

fn main() {
    loop {
        let next_run = find_next_run();
        std::thread::sleep(Duration::from_secs(next_run.1));
        let c = check_job(&next_run.0);
        match c {
            Ok(_) => { println!("Success!") }
            Err(_) => { println!("Error!") }
        }
    }
}
