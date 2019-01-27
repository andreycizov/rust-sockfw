use clap::{App, SubCommand};

use sockfw::*;
use sockfw::args::*;

const TLS: &str = "tls";
const TCP: &str = "tcp";
const UNIX: &str = "unix";

fn parser_for_in<'a, 'b>(app: App<'a, 'b>, x: &str) -> App<'a, 'b> {
    match x {
        TCP => proto::tcp::TcpListener::parser(app),
        TLS => proto::ssl::SslListener::parser(app),
        _ => unreachable!(x)
    }
}

fn parser_for_out<'a, 'b>(app: App<'a, 'b>, x: &str) -> App<'a, 'b> {
    match x {
        UNIX => proto::unix::UnixConnector::parser(app),
        _ => unreachable!(x)
    }
}

fn main() {
    let mut app = App::new("universal forwarder")
        .version("0.1")
        .author("Andrey Cizov <acizov@gmail.com>");

    app = FwConf::parser(app);

    for in_name in vec![TCP, TLS] {
        let mut sc = SubCommand::with_name(in_name);

        sc = parser_for_in(sc, in_name);

        for out_name in vec![UNIX] {
            let mut sco = SubCommand::with_name(out_name);

            sco = parser_for_out(sco, out_name);

            sc = sc.subcommand(sco);
        }

        app = app.subcommand(sc);
    }

    let matches = app.get_matches();

    let conf = FwConf::parse(&matches).unwrap();

    if let Some(matches) = matches.subcommand_matches(TCP) {
        let c_i = proto::tcp::TcpListener::parse(matches).unwrap();

        if let Some(matches) = matches.subcommand_matches(UNIX) {
            let c_o = proto::unix::UnixConnector::parse(matches).unwrap();

            Fw::from_conf(&conf, c_i, c_o).unwrap().run();
        } else {
            unreachable!();
        }
    } else if let Some(matches) = matches.subcommand_matches(TLS) {
        let c_i = proto::ssl::SslListener::parse(matches).unwrap();

        if let Some(matches) = matches.subcommand_matches(UNIX) {
            let c_o = proto::unix::UnixConnector::parse(matches).unwrap();

            Fw::from_conf(&conf, c_i, c_o).unwrap().run();
        } else {
            unreachable!();
        }
    } else {
        eprintln!("invalid command {:?}", matches);
        ::std::process::exit(-1);
    }
}
