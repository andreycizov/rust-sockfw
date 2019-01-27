use std::time::Duration;
use crate::args::Parsable;
use clap::{Arg, App};
use clap::ArgMatches;

#[derive(Debug, Clone)]
pub struct StreamConf {
    pub linger: Option<Duration>,
    pub keepalive: Option<Duration>,
}

impl Parsable<Result<StreamConf, &'static str>> for StreamConf {
    fn parser<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
        app
            .arg(
                Arg::with_name("keepalive_ms")
                    .long("keepalive")
                    .help("setting the value to 0 disables the setting")
                    .default_value("5000")
            )
            .arg(
                Arg::with_name("linger_ms")
                    .long("linger")
                    .help("setting the value to 0 disables the setting")
                    .default_value("2000")
            )
    }

    fn parse(matches: &ArgMatches) -> Result<StreamConf, &'static str> {

        let keepalive = matches.value_of("keepalive_ms").ok_or("keepalive not found")?;
        let keepalive = keepalive.parse::<u64>().map_err(|_| "keepalive not int")?;
        let keepalive = if keepalive == 0 {
            None
        } else {
            Some(Duration::from_millis(keepalive))
        };

        let linger = matches.value_of("linger_ms").ok_or("linger not found")?;
        let linger = linger.parse::<u64>().map_err(|_| "linger not int")?;
        let linger = if linger == 0 {
            None
        } else {
            Some(Duration::from_millis(linger))
        };

        Ok(
            StreamConf { linger, keepalive }
        )
    }
}