use case::CaseExt;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens, TokenStreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn main() {
    let protocol: Protocol = serde_yaml::from_reader(std::io::stdin()).unwrap();
    let t = protocol.into_token_stream();
    println!("{}\n// vim: foldnestmax=0 ft=rust", t);
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct Protocol(HashMap<String, Node>);

#[derive(Debug, PartialEq, Clone)]
enum Node {
    Enum(Enum),
    Object(Object),
    /// fields embedded in others
    Mixin(Object),
    Interface(Interface)
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>
    {
        #[derive(Debug, Deserialize, PartialEq)]
        struct A {
            r#type: String,
            commands: Option<Commands>,
            events: Option<Events>,
            extends: Option<String>,
            initializer: Option<Properties>,
            literals: Option<Vec<String>>,
            properties: Option<Properties>
        }
        let a = A::deserialize(deserializer)?;
        Ok(match &*a.r#type {
            "object" => Self::Object(Object {
                properties: a.properties.unwrap()
            }),
            "mixin" => Self::Mixin(Object {
                properties: a.properties.unwrap()
            }),
            "enum" => Self::Enum(Enum {
                literals: a.literals.unwrap()
            }),
            "interface" => {
                let A {
                    commands,
                    events,
                    extends,
                    initializer,
                    ..
                } = a;
                Self::Interface(Interface {
                    commands,
                    events,
                    extends,
                    initializer
                })
            }
            _ => unreachable!()
        })
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Enum {
    literals: Vec<String>
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Interface {
    commands: Option<Commands>,
    events: Option<Events>,
    extends: Option<String>,
    initializer: Option<Properties>
}

/// ex. {"close": null}
type Commands = HashMap<String, Option<Command>>;
type Events = HashMap<String, Option<Event>>;
type Properties = HashMap<String, Type>;

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(transparent)]
struct Object {
    properties: Properties
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Command {
    parameters: Option<Properties>,
    returns: Option<Properties>,
    // True as String
    #[serde(default)]
    experimental: Option<String>,
    tracing: Option<Tracing>
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
struct Tracing {
    snapshot: bool,
    pause_before_input: Option<bool>
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Event {
    parameters: Properties
}

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize)]
#[serde(untagged)]
enum Type {
    Name(String),
    Items {
        r#type: String,
        #[serde(rename = "items")]
        item_type: Box<Type>
    },
    Literals {
        r#type: String,
        literals: Vec<String>
    },
    Properties {
        r#type: String,
        properties: Properties
    }
}

impl ToTokens for Protocol {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut xs = self.0.iter().collect::<Vec<_>>();
        xs.sort_by_key(|(_, n)| match n {
            Node::Enum(_) => 2,
            Node::Object(_) => 4,
            Node::Mixin(_) => 3,
            Node::Interface(_) => 1
        });
        tokens.append_all(xs.into_iter().map(|(name, node)| node_tokens(name, node)));
    }
}

fn node_tokens(name: &str, node: &Node) -> TokenStream {
    match node {
        Node::Enum(x) => enum_tokens(name, x, true),
        Node::Object(x) | Node::Mixin(x) => object_tokens(name, x),
        Node::Interface(_) => quote!()
    }
}

fn enum_tokens(name: &str, x: &Enum, camel: bool) -> TokenStream {
    let variants = x
        .literals
        .iter()
        .map(|label| {
            let orig = camel.then(|| quote!(#[rename=#label])).unwrap_or_default();
            let label = format_ident!("{}", label.to_camel());
            quote! {
                #orig
                #label
            }
        })
        .collect::<Vec<_>>();
    let ignore_warn = (!camel)
        .then(|| quote!(#[allow(non_camel_case_types)]))
        .unwrap_or_default();
    let name = format_ident!("{}", name);
    quote! {
        #[derive(Debug, Serialize, Deserialize)]
        #ignore_warn
        pub enum #name {
            #(#variants),*
        }
    }
}

fn object_tokens(name: &str, x: &Object) -> TokenStream {
    let nodes = collect_unnamed_by_properties(vec![name], &x.properties);
    quote! {}
}

fn interface_tokens(name: &str, x: &Interface) -> TokenStream {
    let Interface {
        commands,
        events,
        extends,
        initializer
    } = x;
    quote! {}
}

fn collect_unnamed_by_properties<'a>(
    prefix: Vec<&str>,
    props: &'a Properties
) -> HashMap<String, Node> {
    let mut res = HashMap::new();
    for (name, t) in props.iter() {
        res.extend(collect_unnamed(&prefix, name, t));
    }
    res
}

fn collect_unnamed<'a>(prefix: &[&str], name: &str, ty: &'a Type) -> HashMap<String, Node> {
    let mut res = HashMap::new();
    match ty {
        Type::Name(_) => {}
        Type::Items { r#type, item_type } => {
            assert!(
                r#type == "array" || r#type == "array?",
                "Not match Type::Items"
            );
            res.extend(collect_unnamed(prefix, name, &item_type));
        }
        Type::Literals { r#type, literals } => {
            assert!(
                r#type == "enum" || r#type == "enum?",
                "Not match Type::Literals"
            );
            res.insert(
                prefix
                    .iter()
                    .map(|s| -> &str { s })
                    .chain(std::iter::once(&name as &str))
                    .map(|s| s.to_camel())
                    .collect(),
                Node::Enum(Enum {
                    literals: literals.clone()
                })
            );
        }
        Type::Properties { r#type, properties } => {
            assert!(
                r#type == "object" || r#type == "object?",
                "Not match Type::Properties"
            );
            res.insert(
                prefix
                    .iter()
                    .map(|s| -> &str { s })
                    .chain(std::iter::once(&name as &str))
                    .map(|s| s.to_camel())
                    .collect(),
                Node::Object(Object {
                    properties: properties.clone()
                })
            );
            res.extend(collect_unnamed_by_properties(
                {
                    let mut p = prefix.to_vec();
                    p.push(name);
                    p
                },
                properties
            ));
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn all_types() {
        let s = fs::read_to_string("../src/protocol/protocol.yml").unwrap();
        let s = s.replace("null", r#""null""#);
        let protocol: Protocol = serde_yaml::from_str(&s).unwrap();
        let mut types = Vec::new();
        fn add<'a>(dest: &mut Vec<&'a Type>, t: &'a Type) {
            match t {
                Type::Name(_) => dest.push(t),
                Type::Items { item_type, .. } => add(dest, &item_type),
                Type::Literals { .. } => {}
                Type::Properties { properties, .. } => {
                    for (_, t) in properties.iter() {
                        add(dest, t);
                    }
                }
            }
        }
        for (_, node) in protocol.0.iter() {
            match node {
                Node::Object(Object { properties }) | Node::Mixin(Object { properties }) => {
                    for (_, t) in properties {
                        add(&mut types, t);
                    }
                }
                Node::Interface(Interface {
                    commands,
                    events,
                    extends: _,
                    initializer
                }) => {
                    fn append<'a>(dest: &mut Vec<&'a Type>, props: &'a Option<Properties>) {
                        for (_, t) in props.iter().flat_map(|m| m.iter()) {
                            add(dest, t);
                        }
                    }
                    for (_, c) in commands.iter().flat_map(|m| m.iter()) {
                        let c = if c.is_none() {
                            continue;
                        } else {
                            c.as_ref().unwrap()
                        };
                        append(&mut types, &c.parameters);
                        append(&mut types, &c.returns);
                    }
                    for (_, e) in events.iter().flat_map(|m| m.iter()) {
                        let e = if e.is_none() {
                            continue;
                        } else {
                            e.as_ref().unwrap()
                        };
                        for (_, t) in e.parameters.iter() {
                            add(&mut types, t);
                        }
                    }
                    append(&mut types, initializer);
                }
                _ => {}
            }
        }
        println!("{}", serde_json::to_string(&types).unwrap());
    }
}