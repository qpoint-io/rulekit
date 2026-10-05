//! The standard library functions, defined with the typed function API.

use crate::error::Error;
use crate::func::{Args, CallFailure, FnError, FromArg, Param, invoke};
use crate::value::{TextForm, Val};

/// Arguments of `starts_with(value, prefix)`: strings or values with a text
/// form. (What `#[derive(Args)]` generates, written out so the standard
/// library does not need the `derive` feature.)
struct StartsWithArgs<'a> {
    value: TextForm<'a>,
    prefix: TextForm<'a>,
}

impl Args for StartsWithArgs<'static> {
    type Of<'a> = StartsWithArgs<'a>;
    const PARAMS: &'static [Param] = &[
        Param::new("value", <TextForm<'static> as FromArg<'static>>::TYPE),
        Param::new("prefix", <TextForm<'static> as FromArg<'static>>::TYPE),
    ];
    #[inline(always)]
    fn parse<'a>(vals: &'a [Val<'a>]) -> Result<StartsWithArgs<'a>, Error> {
        Ok(StartsWithArgs {
            value: crate::func::__private::arg(vals, 0, "value")?,
            prefix: crate::func::__private::arg(vals, 1, "prefix")?,
        })
    }
}

fn starts_with(_: &(), args: StartsWithArgs<'_>) -> Result<bool, FnError> {
    Ok(args.value.starts_with(&*args.prefix))
}

/// The parameters of a standard library function, if `name` is one.
pub(crate) fn params(name: &str) -> Option<&'static [Param]> {
    match name {
        "starts_with" => Some(StartsWithArgs::PARAMS),
        _ => None,
    }
}

/// Call `starts_with` with evaluated arguments.
#[inline(always)]
pub(crate) fn call_starts_with<'a>(vals: &[Val<'_>]) -> Result<Val<'a>, CallFailure> {
    invoke::<(), StartsWithArgs, bool, _>(&starts_with, &(), vals)
        .map_err(|failure| failure.named("starts_with"))
}
