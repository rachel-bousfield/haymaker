//
// Haymaker
//

use crate::comments::uncomment;
use crate::console::Color;
use crate::derive::{add_derivation_highlights, derive, VarMap};
use crate::line::LineInfo;
use crate::parsed::MakeLine;
use crate::recipe::Recipe;
use crate::text::Text;

use itertools::Itertools;
use petgraph::{stable_graph::StableGraph, Direction};
use std::path::Path;
use std::path::PathBuf;
use structopt::StructOpt;

use lalrpop_util::lalrpop_mod;
//use lalrpop_util::ParseError;
use crate::def::DefParser;
lalrpop_mod!(def);

mod comments;
mod console;
mod derive;
mod line;
mod parsed;
mod recipe;
mod regexes;
mod text;

#[derive(Debug, StructOpt)]
#[structopt(name = "haymaker", about = "A fearlessly parallel build system")]
struct Opt {
    #[structopt(parse(from_os_str))]
    hayfile: Option<PathBuf>,
}

fn main() {
    let opt = Opt::from_args();

    let hayfile = match opt.hayfile {
        Some(hayfile) => hayfile,
        None => {
            let defaults = ["hayfile", "Hayfile", "makefile", "Makefile"];

            match defaults.into_iter().find(|file| Path::new(file).exists()) {
                Some(hayfile) => Path::new(hayfile).to_path_buf(),
                None => {
                    println!("No {} in current directory", "hayfile".red());
                    std::process::exit(1);
                }
            }
        }
    };

    let filename = hayfile.to_string_lossy();
    let haysource = match std::fs::read_to_string(&hayfile) {
        Ok(haysource) => haysource,
        Err(err) => {
            println!("Could not open {}\n{}", filename.red(), err);
            std::process::exit(1);
        }
    };

    let mut recipes: Vec<Recipe> = vec![];
    let mut vars = VarMap::new();
    let lines = uncomment(&haysource, "");

    for (index, line) in lines.into_iter().enumerate() {
        // Hayfiles are context-sensitive, so we must determine how to handle each line

        let lineno = index + 1;
        let source = &line;

        if line.trim() == "" {
            // skip blanks for performance
            continue;
        }

        let info = LineInfo::from(line.as_ref());
        let line = info.sans_flags.trim();

        if info.shell {
            // shell source can have arbitrary text & starts after the tab

            let recipe = match recipes.last_mut() {
                Some(recipe) => recipe,
                None => {
                    let kind = "Structure";
                    let message = "stray shell code outside of a recipe";
                    console::print_source_error(
                        kind, &message, &filename, &source, lineno, info.split,
                    );
                    std::process::exit(1);
                }
            };

            recipe.add_command(line.to_string(), info.debug);
            continue;
        }

        if line.contains("=") {
            // variable assignments

            let sides = line.split('=').into_iter().rev();

            for (value, dest) in sides.tuple_windows() {
                let value = value.trim();
                let assigns = regexes::VAR.captures_iter(dest).map(|x| x[0].to_string());

                for assign in assigns {
                    vars.insert(assign, value.to_string());
                }
            }
            continue;
        }

        let raw = line.clone();
        let line = match derive(&line, &mut vars, info.debug) {
            Ok(line) => line,
            Err(message) => {
                let note = format!("{}: {} {}", "note".white(), "this was", raw.grey());
                let help = format!(
                    "{}: place a {} before the line to enable debug mode",
                    "help".white(),
                    "+".mint()
                );

                let more = match info.debug {
                    true => vec![note],
                    false => vec![note, help],
                };
                let kind = "Subcall";
                console::print_processed_error(
                    kind, &message, &filename, &line, more, lineno, 0,
                );
                
                if !info.neglect {
                    std::process::exit(1);
                }
                continue;
            }
        };

        if line.starts_with("include") {
            //

            let mut includes = line.split_when_balanced_with_offsets(' ', '\'').into_iter();
            includes.next(); // discard the "import"

            for (mut offset, include) in includes {
                //

                if !Path::new(include).exists() {
                    let kind = "Include";
                    let message = format!("file {} does not exist", include.red());

                    if line == raw {
                        offset += info.split;
                        console::print_source_error(
                            kind, &message, &filename, &source, lineno, offset,
                        );
                    } else {
                        let note = format!("{}: {} {}", "note".white(), "this was", raw.grey());
                        let help = format!(
                            "{}: place a {} before the line to enable debug mode",
                            "help".white(),
                            "+".mint()
                        );

                        let info = match info.debug {
                            true => vec![note],
                            false => vec![note, help],
                        };

                        console::print_processed_error(
                            kind, &message, &filename, &line, info, lineno, offset,
                        );
                    }

                    if !info.neglect {
                        std::process::exit(1);
                    }
                }
            }
            continue;
        }

        let parsed = match DefParser::new().parse(&line) {
            Ok(Some(parsed)) => parsed,
            Err(err) => panic!("error parsing\n{}\n{}", line.red(), err),
            Ok(_) => continue,
        };

        let MakeLine::Rule(rule) = parsed;
        let recipe = Recipe::from(rule);
        recipes.push(recipe);
    }

    for (variable, value) in &vars {
        println!("{} {} {}", variable, "≡".pink(), add_derivation_highlights(value));
    }
    println!();

    for recipe in &recipes {
        recipe.print();
        println!();
    }

    let mut graph: StableGraph<Recipe, ()> = StableGraph::new();
    //let mut nodes: BTreeMap::new();

    for recipe in recipes {
        let _node = graph.add_node(recipe);
    }

    while graph.node_count() > 0 {
        let ready: Vec<_> = graph.externals(Direction::Outgoing).collect();

        for node in ready {
            let recipe = &graph[node];
            recipe.execute(&vars);
            graph.remove_node(node);
        }
    }
}
