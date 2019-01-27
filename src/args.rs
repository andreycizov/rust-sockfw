use clap::{App, ArgMatches};


trait ArgMatchesExt<R> {
    fn parse(self) -> R;
}

trait AppExt<R, T: Parsable<R>> {
    fn parser(self) -> Self;
}

impl<'a, R: Parsable<R>> ArgMatchesExt<R> for ArgMatches<'a> {
    fn parse(self) -> R {
        R::parse(&self)
    }
}

impl<'a, 'b, R, T: Parsable<R>> AppExt<R, T> for App<'a, 'b> {
    fn parser(self) -> Self {
        return T::parser(self);
    }
}

trait Parsable<R> {
    fn parser<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b>;
    fn parse<'a>(matches: &ArgMatches) -> R;
}


