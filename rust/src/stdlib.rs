//! The standard library functions, defined with the typed function API.

use crate::error::Error;
use crate::func::{Args, CallFailure, FromArg, Param};
use crate::value::{ArrayRef, TextForm, Val, ValueRef};

/// Arguments of `starts_with(value, prefix)`. (What `#[derive(Args)]`
/// generates, written out so the standard library does not need the
/// `derive` feature.)
struct StartsWithArgs<'a> {
    value: TextForm<'a>,
    prefix: Prefix<'a>,
}

impl Args for StartsWithArgs<'static> {
    type Of<'a> = StartsWithArgs<'a>;
    const PARAMS: &'static [Param] = &[
        Param::new("value", <TextForm<'static> as FromArg<'static>>::TYPE),
        Param::new("prefix", <Prefix<'static> as FromArg<'static>>::TYPE),
    ];
    #[inline(always)]
    fn parse<'a>(vals: &'a [Val<'a>]) -> Result<StartsWithArgs<'a>, Error> {
        Ok(StartsWithArgs {
            value: crate::func::__private::arg(vals, 0, "value")?,
            prefix: crate::func::__private::arg(vals, 1, "prefix")?,
        })
    }
}

/// The `prefix` of `starts_with`: a string or a value with a text form, or a
/// list of them (borrowed; its items are converted as they are checked).
enum Prefix<'a> {
    One(TextForm<'a>),
    List(ArrayRef<'a>),
}

impl<'a> FromArg<'a> for Prefix<'a> {
    /// A list is accepted too, but its items must be strings.
    const TYPE: &'static str = <TextForm<'a> as FromArg<'a>>::TYPE;
    #[inline(always)]
    fn from_arg(value: ValueRef<'a>) -> Option<Self> {
        match value {
            ValueRef::Array(list) => Some(Prefix::List(list)),
            value => value.text().map(Prefix::One),
        }
    }
}

/// Whether `value` starts with `prefix`, or with any prefix of a list.
#[inline(always)]
fn starts_with(args: StartsWithArgs<'_>) -> Result<bool, Error> {
    match args.prefix {
        Prefix::One(prefix) => Ok(args.value.starts_with(&*prefix)),
        Prefix::List(list) => starts_with_any(&args.value, list),
    }
}

/// Whether `value` starts with any prefix of `list`, checked in order up to
/// the first match. An item without a text form is an [`Error::InvalidArg`],
/// like a single prefix of the wrong type. (Not inlined, so the list loop
/// does not slow down the single-prefix call.)
#[inline(never)]
fn starts_with_any(value: &str, list: ArrayRef<'_>) -> Result<bool, Error> {
    for item in list.iter() {
        let Some(prefix) = item.text() else {
            return Err(invalid_prefix(item));
        };
        if value.starts_with(&*prefix) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cold]
fn invalid_prefix(item: ValueRef<'_>) -> Error {
    Error::InvalidArg {
        name: "prefix".to_owned(),
        expected: <Prefix<'static> as FromArg<'static>>::TYPE.to_owned(),
        got: item.type_name().to_owned(),
    }
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
    StartsWithArgs::parse(vals)
        .and_then(starts_with)
        .map(|pass| Val::Ref(ValueRef::Bool(pass)))
        .map_err(CallFailure::Error)
}
