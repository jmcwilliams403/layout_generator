use std::io::Write;
use clap::{arg, command, Command};
use oxeylyzer::{
    generate::LayoutGeneration,
    layout::*,
    weights::Config,
    load_text,
    language_data::LanguageData
};

pub struct Repl {
    language: String,
    gen: LayoutGeneration,
    pins: Vec<usize>
}

impl Repl {
    pub fn run() -> Result<(), String> {
        let config = Config::new();

        let mut env = Self {
            language: config.defaults.language.clone(),
            gen: LayoutGeneration::new(
                config.defaults.language.as_str(),
                config.trigram_precision(),
                Some(config.weights),
            ).expect(format!("Could not read language data for {}", config.defaults.language).as_str()),
            pins: config.pins
        };

        loop {
            let line = readline()?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match env.respond(line) {
                Ok(quit) => {
                    if quit {
                        break;
                    }
                }
                Err(err) => {
                    write!(std::io::stdout(), "{}", err).map_err(|e| e.to_string())?;
                    std::io::stdout().flush().map_err(|e| e.to_string())?;
                }
            }
        }

        Ok(())
    }

    fn language_data(&self) -> &LanguageData {
		&self.gen.analysis.language_data
	}

    fn get_ngram_info(&self, ngram: &str) -> String {
        match ngram.chars().count() {
            1 => {
                let c = ngram.chars().next().unwrap();
                let occ = self.language_data().characters.get(&c).unwrap_or(&0.0) * 100.0;
                format!("{ngram}: {occ:.3}%")
            },
            2 => {
                let b: [char; 2] = ngram.chars().collect::<Vec<char>>().try_into().unwrap();
                let b2 = [b[1], b[0]];
                let rev = String::from_iter(b2);
                let occ_b = self.language_data().bigrams.get(&b).unwrap_or(&0.0) * 100.0;
                let occ_b2 = self.language_data().bigrams.get(&b2).unwrap_or(&0.0) * 100.0;
                let occ_s = self.language_data().skipgrams.get(&b).unwrap_or(&0.0) * 100.0;
                let occ_s2 = self.language_data().skipgrams.get(&b2).unwrap_or(&0.0) * 100.0;
                format!(
                    "{ngram} + {rev}: {:.3}%,\n  {ngram}: {occ_b:.3}%\n  {rev}: {occ_b2:.3}%\n\
                    {ngram} + {rev} (skipgram): {:.3}%,\n  {ngram}: {occ_s:.3}%\n  {rev}: {occ_s2:.3}%",
                    occ_b+occ_b2, occ_s+occ_s2
                )
            }
            3 => {
                let t: [char; 3] = ngram.chars().collect::<Vec<char>>().try_into().unwrap();
                let &(_, occ) = self.language_data().trigrams
                    .iter()
                    .find(|&&(tf, _)| tf == t)
                    .unwrap_or(&(t, 0.0));
                format!("{ngram}: {:.3}%", occ*100.0)
            }
            _ => "Invalid ngram! It must be 1, 2 or 3 chars long.".to_string()
        }
    }

    fn get_nth(&self, nr: usize) -> Option<FastLayout> {
        if let Some(temp_list) = &self.gen.temp_generated {
            if nr < temp_list.len() {
                let l = FastLayout::try_from(temp_list[nr].as_str()).unwrap();
                Some(l)
            } else {
                println!("That's not a valid index!");
                None
            }
        } else {
            println!("You haven't generated any layouts yet!");
            None
        }
    }

    fn save(&mut self, save_m: &clap::ArgMatches) {
        let n_str = save_m.value_of("NR").unwrap();
        if let Ok(nr) = usize::from_str_radix(n_str, 10) {
            if let Some(layout) = self.get_nth(nr) {
                if let Some(name) = save_m.value_of("NAME") {
                    self.gen.analysis.save(layout, Some(name.to_string())).unwrap();
                } else {
                    self.gen.analysis.save(layout, None).unwrap();
                }
            }
        }
    }

    fn respond(&mut self, line: &str) -> Result<bool, String> {
        let args = shlex::split(line).ok_or("error: Invalid quoting")?;
        let matches = self.cli()
            .try_get_matches_from(&args)
            .map_err(|e| e.to_string())?;
        match matches.subcommand() {
            Some(("generate", new_m)) => {
                let count_str = new_m.value_of("COUNT").unwrap();
                println!("generating {} layouts...", count_str);
                let count = usize::from_str_radix(count_str, 10).map_err(|e| e.to_string())?;
                self.gen.generate_n(count);
            }
            Some(("improve", comp_m)) => {
                let name = comp_m.value_of("LAYOUT_NAME").unwrap();
                let amount_str = comp_m.value_of("AMOUNT").unwrap();
                if let Ok(amount) = usize::from_str_radix(amount_str, 10) {
                    if let Some(l) = self.gen.analysis.layout_by_name(name) {
                        self.gen.generate_n_pins(amount, l.clone(), &self.pins);
                    }
                }
            }
            Some(("rank", _)) => {
                self.gen.analysis.rank();
            }
            Some(("analyze", new_m)) => {
                let name_or_nr = new_m.value_of("LAYOUT_NAME_OR_NR").unwrap();
                if let Ok(nr) = usize::from_str_radix(name_or_nr, 10) {
                    if let Some(layout) = self.get_nth(nr) {
                        self.gen.analysis.analyze(&layout);
                    }
                } else {
                    self.gen.analysis.analyze_name(name_or_nr);
                }
            }
            Some(("ngram", occ_m)) => {
                let ngram = occ_m.value_of("NGRAM").unwrap();
                println!("{}", self.get_ngram_info(ngram));
            }
            Some(("compare", new_m)) => {
                let layout1 = new_m.value_of("LAYOUT_1").unwrap();
                let layout2 = new_m.value_of("LAYOUT_2").unwrap();
                self.gen.analysis.compare_name(layout1, layout2);
            }
            Some(("language", lang_m)) => {
                let config = Config::new();

                match lang_m.value_of("LANGUAGE") {
                    Some(language) => {
                        if let Ok(generator) = LayoutGeneration::new(
                            language, config.trigram_precision(), Some(config.weights)
                        ) {
                            self.language = language.to_string();
                            self.gen = generator;
                            println!("Set language to {}", language);
                        } else {
                            println!("Could not load {}", language);
                        }
                    },
                    None => println!("Current language: {}", self.language)
                }
            }
            Some(("languages", _)) => {
                for entry in std::fs::read_dir("static/language_data").unwrap() {
                    if let Ok(p) = entry {
                        let name = p
                            .file_name()
                            .to_string_lossy()
                            .replace("_", " ")
                            .replace(".json", "");
                        if name != "test" {
                            println!("{}", name);
                        }
                    }
                }
            }
            Some(("reload", _)) => {
                let config = Config::new();

                if let Ok(generator) = LayoutGeneration::new(
                    self.language.as_str(), config.trigram_precision(), Some(config.weights)
                ) {
                    self.gen = generator;
                self.pins = config.pins;
                } else {
                    println!("Could not load {}", self.language);
                }
            }
            Some(("save", save_m)) => {
                self.save(save_m);
            }
            Some(("load", load_m)) => {
                if let Some(language) = load_m.value_of("LANGUAGE") {
                    load_text::load_default(language);
                }
            }
            Some(("quit", _)) => {
                println!("Exiting anlyzer...");
                return Ok(true);
            }
            Some((name, _new_m)) => println!("{name} is not a valid command!"),
            None => unreachable!("subcommand required"),
        }

        Ok(false)
    }

    fn cli(&self) -> Command<'static> {
        // strip out usage
        const REPL_TEMPLATE: &str = "\
            {all-args}
        ";
        // strip out name/version
        const COMMAND_TEMPLATE: &str = "\
            {about-with-newline}\n\
            {usage-heading}\n    {usage}\n\
            \n\
            {all-args}{after-help}\
        ";

        command!("repl")
            .multicall(true)
            .arg_required_else_help(true)
            .subcommand_required(true)
            .subcommand_value_name("APPLET")
            .subcommand_help_heading("APPLETS")
            .help_template(REPL_TEMPLATE)
            .subcommand(
                command!("rank")
                    .alias("sort")
                    .about("(sort) Rank all layouts in set language by score using values set from 'config.toml'")
                    .help_template(COMMAND_TEMPLATE),
            )
            .subcommand(
                command!("analyze")
                    .aliases(&["a", "layout"])
                    .arg(
                        arg!(<LAYOUT_NAME_OR_NR>)
                    )
                    .about("(a, layout) Show details of layout")
                    .help_template(COMMAND_TEMPLATE)
            )
            .subcommand(
                command!("compare")
                    .aliases(&["c", "comp", "cmopare", "comprae"])
                    .arg(
                        arg!(<LAYOUT_1>)
                    )
                    .arg(
                        arg!(<LAYOUT_2>)
                    )
                    .about("(c, comp) Compare 2 layouts")
                    .help_template(COMMAND_TEMPLATE)
            )
            .subcommand(
                command!("language")
                    .aliases(&["l", "lang", "lanugage", "langauge"])
                    .arg(
                        arg!([LANGUAGE])
                    )
                    .help_template(COMMAND_TEMPLATE)
                    .about("(l, lang) Set a language to be used for analysis. Loads corpus when not present")
            )
            .subcommand(
                command!("languages")
                .aliases(&["langs", "lanugages", "langauges"])
                .help_template(COMMAND_TEMPLATE)
                .about("(langs) Show available languages")
            )
            .subcommand(
                command!("ngram")
                .aliases(&["n","occ"])
                .help_template(COMMAND_TEMPLATE) 
                .arg(
                        arg!(<NGRAM>)
                )
                .about("(occ) Gives information about a certain ngram. for 2 letter ones, skipgram info will be provided as well.")
            )
            .subcommand(
                command!("reload")
                .alias("r")
                .help_template(COMMAND_TEMPLATE)
                .about("(r) Reloads all data with the current language. Loses temporary layouts. ")
            )
            .subcommand(
                command!("generate")
                    .aliases(&["g", "gen"])
                    .arg(
                        arg!(<COUNT>)
                    )
                    .help_template(COMMAND_TEMPLATE)
                    .about("(g, gen) Generate a number of layouts and shows the best 10, All layouts generated are accessible until reloading or quiting. ")
            )
            .subcommand(
                command!("improve")
                    .aliases(&["i", "optimize"])
                    .arg(
                        arg!(<LAYOUT_NAME>)
                    )
                    .arg(
                        arg!(<AMOUNT>)
                    )
                    .help_template(COMMAND_TEMPLATE)
                    .about("(i, optimize) Save the top <NR> result that was generated. Starts from 1, Takes negative values")
            )
            .subcommand(
                command!("save")
                .alias("s")
                .arg(
                    arg!(<NR>)
                )
                .arg(
                    arg!([NAME])
                )
                .help_template(COMMAND_TEMPLATE)
                .about("(s) Save the top <NR> result that was generated. Starts from 1 up to the number generated, Takes negative values")
            )
            .subcommand(
                command!("load")
                .arg(
                    arg!(<LANGUAGE>)
                )
                .help_template(COMMAND_TEMPLATE)
                .about("Generates corpus for <language>. Will be exclude spaces from source if the language isn't known")
            )
            // .subcommand(
            //     command!("passthrough")
            //     .alias("pass")
            //     .arg(
            //         arg!(<LANGUAGE>)
            //     )
            //     .help_template(COMMAND_TEMPLATE)
            //     .about("Loads corpus as passthrough for <language> in static/language_data_pass")
            // )
            .subcommand(
                command!("quit")
                    .aliases(&["exit","q"])
                    .about("(q) Quit the repl")
                    .help_template(COMMAND_TEMPLATE),
            )
    }
}

fn readline() -> Result<String, String> {
    write!(std::io::stdout(), "> ").map_err(|e| e.to_string())?;
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut buf = String::new();
    std::io::stdin()
        .read_line(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}
