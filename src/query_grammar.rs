use tokenizer::TokenStream;

use combine::{parser, ParseResult, Parser};
use combine::easy::Error;
use combine::error::StreamError;
use combine::combinator::{many, many1, eof, optional};

use query_error::{QueryParseError};
use tokenizer::{Kind as T, Token};
use helpers::{punct, ident, kind, name};
use query::*;

pub fn empty_selection() -> SelectionSet {
    SelectionSet { items: Vec::new() }
}

pub fn directives<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Vec<Directive>, TokenStream<'a>>
{
    many(punct("@")
        .with(name())
        .and(parser(arguments))
        .map(|(name, arguments)| Directive { name, arguments }))
    .parse_stream(input)
}

pub fn arguments<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Vec<(String, Value)>, TokenStream<'a>>
{
    optional(
        punct("(")
        .with(many1(name()
            .skip(punct(":"))
            .and(parser(value))))
        .skip(punct(")")))
    .map(|opt| {
        opt.unwrap_or_else(Vec::new)
    })
    .parse_stream(input)
}

pub fn field<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Field, TokenStream<'a>>
{
    name()
    .and(optional(punct(":").with(name())))
    .and(parser(arguments))
    .and(parser(directives))
    .and(optional(parser(selection_set)))
    .map(|((((name_or_alias, opt_name), arguments), directives), sel)| {
        let (name, alias) = match opt_name {
            Some(name) => (name, Some(name_or_alias)),
            None => (name_or_alias, None),
        };
        Field {
            name, alias, arguments, directives,
            selection_set: sel.unwrap_or_else(empty_selection),
        }
    })
    .parse_stream(input)
}

pub fn selection<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Selection, TokenStream<'a>>
{
    parser(field).map(Selection::Field)
    .or(punct("...").with(
        optional(ident("on").with(name()).map(TypeCondition::On))
            .and(parser(directives))
            .and(parser(selection_set))
            .map(|((type_condition, directives), selection_set)| {
                InlineFragment { type_condition, selection_set, directives }
            })
            .map(Selection::InlineFragment)
        .or(name()
            .and(parser(directives))
            .map(|(fragment_name, directives)| {
                FragmentSpread { fragment_name, directives }
            })
            .map(Selection::FragmentSpread))
    ))
    .parse_stream(input)
}

pub fn selection_set<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<SelectionSet, TokenStream<'a>>
{
    punct("{")
    .with(many1(parser(selection)))
    .skip(punct("}"))
    .map(|items| SelectionSet { items })
    .parse_stream(input)
}

pub fn variable_type<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<VariableType, TokenStream<'a>>
{
    name().map(|x| VariableType::NamedType(x))
    // .or(list...)
    // .or(non_null_type)
    .parse_stream(input)
}

pub fn int_value<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Value, TokenStream<'a>>
{
    kind(T::IntValue).and_then(|tok| tok.value.parse())
            .map(Number).map(Value::Int)
    .parse_stream(input)
}

fn unquote_string(s: &str) -> Result<String, Error<Token, Token>> {
    let mut res = String::with_capacity(s.len());
    debug_assert!(s.starts_with("\"") && s.ends_with("\""));
    let mut chars = s[1..s.len()-1].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                match chars.next().expect("slash cant be and the end") {
                    c@'"' | c@'\\' | c@'/' => res.push(c),
                    'b' => res.push('\u{0010}'),
                    'f' => res.push('\u{000C}'),
                    'n' => res.push('\n'),
                    'r' => res.push('\r'),
                    't' => res.push('\t'),
                    'u' => {
                        unimplemented!();
                    }
                    c => {
                        return Err(Error::unexpected_message(
                            format_args!("bad escaped char {:?}", c)));
                    }
                }
            }
            c => res.push(c),
        }
    }
    return Ok(res);
}

fn unquote_block_string(s: &str) -> Result<String, Error<Token, Token>> {
    debug_assert!(s.starts_with("\"\"\"") && s.ends_with("\"\"\""));
    let indent = s[3..s.len()-3].lines().skip(1)
        .map(|l| l.len() - l.trim_left().len())
        .min().unwrap_or(0);
    let mut result = String::with_capacity(s.len());
    let mut lines = s[3..s.len()-3].lines();
    if let Some(first) = lines.next() {
        let stripped = first.trim();
        if stripped.len() > 0 {
            result.push_str(stripped);
            result.push('\n');
        }
    }
    for line in lines {
        result.push_str(&line[indent..].replace(r#"\""""#, r#"""""#));
        result.push('\n');
    }
    let trunc_len = result.trim_right().len();
    result.truncate(trunc_len);
    return Ok(result);
}

pub fn string_value<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Value, TokenStream<'a>>
{
    kind(T::StringValue).and_then(|tok| unquote_string(tok.value))
        .map(Value::String)
    .parse_stream(input)
}

pub fn block_string_value<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Value, TokenStream<'a>>
{
    kind(T::BlockString).and_then(|tok| unquote_block_string(tok.value))
        .map(Value::String)
    .parse_stream(input)
}

pub fn value<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Value, TokenStream<'a>>
{
    name().map(Value::EnumValue)
    .or(parser(int_value))
    .or(parser(string_value))
    .or(parser(block_string_value))
    .or(punct("$").with(name()).map(Value::Variable))
    .or(punct("[").with(many(parser(value))).skip(punct("]"))
        .map(|lst| Value::ListValue(lst)))
    .or(punct("{")
        .with(many(name().skip(punct(":")).and(parser(value))))
        .skip(punct("}"))
        .map(|lst| Value::ObjectValue(lst)))
    // TODO(tailhook) more values
    .parse_stream(input)
}

pub fn default_value<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Value, TokenStream<'a>>
{
    name().map(Value::EnumValue)
    .or(parser(int_value))
    .or(parser(block_string_value))
    .or(punct("[").with(many(parser(default_value))).skip(punct("]"))
        .map(|lst| Value::ListValue(lst)))
    // TODO(tailhook) more values
    .parse_stream(input)
}

pub fn query<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Query, TokenStream<'a>>
{
    ident("query")
    .with(parser(operation_common))
    .map(|(name, variable_definitions, selection_set)| Query {
        name, selection_set, variable_definitions,
        directives: Vec::new(),
    })
    .parse_stream(input)
}

pub fn operation_common<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<(Option<String>, Vec<VariableDefinition>, SelectionSet), TokenStream<'a>>
{
    optional(name())
    .and(optional(
        punct("(")
        .with(many1(
            punct("$").with(name()).skip(punct(":"))
                .and(parser(variable_type))
                .and(optional(
                    punct("=")
                    .with(parser(default_value))))
                .map(|((name, var_type), default_value)| VariableDefinition {
                    name, var_type, default_value,
                })
        ))
        .skip(punct(")")))
        .map(|vars| vars.unwrap_or_else(Vec::new)))
    .and(parser(selection_set))
    .map(|((a, b), c)| (a, b, c))
    .parse_stream(input)
}

pub fn mutation<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Mutation, TokenStream<'a>>
{
    ident("mutation")
    .with(parser(operation_common))
    .map(|(name, variable_definitions, selection_set)| Mutation {
        name, selection_set, variable_definitions,
        directives: Vec::new(),
    })
    .parse_stream(input)
}

pub fn subscription<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Subscription, TokenStream<'a>>
{
    ident("subscription")
    .with(parser(operation_common))
    .map(|(name, variable_definitions, selection_set)| Subscription {
        name, selection_set, variable_definitions,
        directives: Vec::new(),
    })
    .parse_stream(input)
}

pub fn operation_definition<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<OperationDefinition, TokenStream<'a>>
{
    parser(selection_set).map(OperationDefinition::SelectionSet)
    .or(parser(query).map(OperationDefinition::Query))
    .or(parser(mutation).map(OperationDefinition::Mutation))
    .or(parser(subscription).map(OperationDefinition::Subscription))
    .parse_stream(input)
}

pub fn fragment_definition<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<FragmentDefinition, TokenStream<'a>>
{
    ident("fragment")
    .with(name())
    .and(ident("on").with(name()).map(TypeCondition::On))
    .and(parser(directives))
    .and(parser(selection_set))
    .map(|(((name, type_condition), directives), selection_set)| {
        FragmentDefinition {
            name, type_condition, directives, selection_set,
        }
    })
    .parse_stream(input)
}

pub fn definition<'a>(input: &mut TokenStream<'a>)
    -> ParseResult<Definition, TokenStream<'a>>
{
    parser(operation_definition).map(Definition::Operation)
    .or(parser(fragment_definition).map(Definition::Fragment))
    .parse_stream(input)
}

pub fn parse_query(s: &str) -> Result<Document, QueryParseError> {
    let mut tokens = TokenStream::new(s);
    let (doc, _) = many1(parser(definition))
        .map(|d| Document { definitions: d })
        .skip(eof())
        .parse_stream(&mut tokens)
        .map_err(|e| e.into_inner().error)?;
    Ok(doc)
}

#[cfg(test)]
mod test {
    use query::*;
    use super::parse_query;

    fn ast(s: &str) -> Document {
        parse_query(s).unwrap()
    }

    #[test]
    fn one_field() {
        assert_eq!(ast("{ a }"), Document {
            definitions: vec![
                Definition::Operation(OperationDefinition::SelectionSet(
                    SelectionSet {
                        items: vec![
                            Selection::Field(Field {
                                alias: None,
                                name: "a".into(),
                                arguments: Vec::new(),
                                directives: Vec::new(),
                                selection_set: SelectionSet {
                                    items: Vec::new()
                                },
                            }),
                        ],
                    }
                ))
            ],
        });
    }

    #[test]
    fn one_field_roundtrip() {
        assert_eq!(ast("{ a }").to_string(), "{\n  a\n}\n");
    }

    #[test]
    #[should_panic(expected="number too large")]
    fn large_integer() {
        ast("{ a(x: 10000000000000000000000000000 }");
    }
}
