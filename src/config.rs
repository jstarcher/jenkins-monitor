use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use toml;

#[derive(Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub general: ConfigGeneral,
    pub jenkins: ConfigJenkins,
    pub job: Vec<ConfigJob>,
}

#[derive(Deserialize, Default, Debug)]
pub struct ConfigGeneral {
    pub log_level: String,
}

#[derive(Deserialize, Default, Debug)]
pub struct ConfigJenkins {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, Default, Debug)]
pub struct ConfigJob {
    pub name: Option<String>,
    pub schedule: String,
}

pub struct ConfigReader;

impl ConfigReader {
    pub fn make() -> Config {
        debug!("reading config file: config.cfg");

        let mut file = File::open("config.toml").expect("cannot find config file");
        let mut conf = String::new();

        file.read_to_string(&mut conf)
            .expect("cannot read config file");

        debug!("read config file: config.toml");

        toml::from_str(&conf).expect("syntax error in config file")
    }
}