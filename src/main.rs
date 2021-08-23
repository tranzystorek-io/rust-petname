#[macro_use]
extern crate clap;

use petname::Petnames;

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path;
use std::process;
use std::str::FromStr;

use clap::Arg;
use rand::seq::IteratorRandom;

fn main() {
    let matches = app().get_matches();
    match run(matches) {
        Err(Error::Disconnected) => {
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
        Ok(()) => {
            process::exit(0);
        }
    }
}

enum Error {
    Io(io::Error),
    FileIo(path::PathBuf, io::Error),
    Cardinality(String),
    Alliteration(String),
    Disconnected,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Error::Io(ref e) => write!(f, "{}", e),
            Error::FileIo(ref path, ref e) => write!(f, "{}: {}", e, path.display()),
            Error::Cardinality(ref message) => write!(f, "cardinality is zero: {}", message),
            Error::Alliteration(ref message) => write!(f, "cannot alliterate: {}", message),
            Error::Disconnected => write!(f, "caller disconnected / stopped reading"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}

fn app<'a, 'b>() -> clap::App<'a, 'b> {
    clap::App::new("rust-petname")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Generate human readable random names.")
        .after_help(concat!(
            "Based on Dustin Kirkland's petname project ",
            "<https://github.com/dustinkirkland/petname>."
        ))
        .arg(
            Arg::with_name("words")
                .short("w")
                .long("words")
                .value_name("WORDS")
                .default_value("2")
                .help("Number of words in name")
                .takes_value(true)
                .validator(can_be_parsed::<u8>),
        )
        .arg(
            Arg::with_name("separator")
                .short("s")
                .long("separator")
                .value_name("SEP")
                .default_value("-")
                .help("Separator between words")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("complexity")
                .short("c")
                .long("complexity")
                .value_name("COM")
                .possible_values(&["0", "1", "2"])
                .hide_possible_values(true)
                .default_value("0")
                .help("Use small words (0), medium words (1), or large words (2)")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("directory")
                .short("d")
                .long("dir")
                .value_name("DIR")
                .help("Directory containing adjectives.txt, adverbs.txt, names.txt")
                .conflicts_with("complexity")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("count")
                .long("count")
                .value_name("COUNT")
                .default_value("1")
                .help(concat!(
                    "Generate multiple names; pass 0 to produce infinite ",
                    "names (--count=0 is deprecated; use --stream instead)"
                ))
                .takes_value(true)
                .validator(can_be_parsed::<usize>),
        )
        .arg(
            Arg::with_name("stream")
                .long("stream")
                .help("Stream names continuously")
                .conflicts_with("count")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("non-repeating")
                .long("non-repeating")
                .help("Do not generate the same name more than once")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("letters")
                .short("l")
                .long("letters")
                .value_name("LETTERS")
                .default_value("0")
                .help("Maxiumum number of letters in each word; 0 for unlimited")
                .takes_value(true)
                .validator(can_be_parsed::<usize>),
        )
        .arg(
            Arg::with_name("alliterate")
                .short("a")
                .long("alliterate")
                .help("Generate names where each word begins with the same letter")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("alliterate-with")
                .long("alliterate-with")
                .short("A")
                .value_name("LETTER")
                .help("Generate names where each word begins with the given letter")
                .takes_value(true)
                .validator(can_be_parsed::<char>),
        )
        .arg(
            // For compatibility with upstream.
            Arg::with_name("ubuntu")
                .short("u")
                .long("ubuntu")
                .help("Alias; see --alliterate")
                .takes_value(false),
        )
}

fn run(matches: clap::ArgMatches) -> Result<(), Error> {
    // Unwrapping is safe because these options have defaults.
    let opt_separator = matches.value_of("separator").unwrap();
    let opt_words = matches.value_of("words").unwrap();
    let opt_complexity = matches.value_of("complexity").unwrap();
    let opt_count = matches.value_of("count").unwrap();
    let opt_letters = matches.value_of("letters").unwrap();

    // Flags.
    let opt_stream = matches.is_present("stream");
    let opt_non_repeating = matches.is_present("non-repeating");
    let opt_alliterate = matches.is_present("alliterate")
        || matches.is_present("ubuntu")
        || matches.is_present("alliterate-with");

    // Optional arguments without defaults.
    let opt_directory = matches.value_of("directory");
    let opt_alliterate_char = matches
        .value_of("alliterate-with")
        .and_then(|s| s.parse::<char>().ok());

    // Parse numbers. Validated so unwrapping is okay.
    let opt_words: u8 = opt_words.parse().unwrap();
    let opt_count: usize = opt_count.parse().unwrap();
    let opt_letters: usize = opt_letters.parse().unwrap();

    // Load custom word lists, if specified.
    let words = match opt_directory {
        Some(dirname) => Words::load(dirname)?,
        None => Words::Builtin,
    };

    // Select the appropriate word list.
    let mut petnames = match words {
        Words::Custom(ref adjectives, ref adverbs, ref names) => {
            Petnames::new(adjectives, adverbs, names)
        }
        Words::Builtin => match opt_complexity {
            "0" => Petnames::small(),
            "1" => Petnames::medium(),
            "2" => Petnames::large(),
            _ => Petnames::small(),
        },
    };

    // If requested, limit the number of letters.
    if opt_letters != 0 {
        petnames.retain(|s| s.len() <= opt_letters);
    }

    // Check cardinality.
    if petnames.cardinality(opt_words) == 0 {
        return Err(Error::Cardinality(
            "no petnames to choose from; try relaxing constraints".to_string(),
        ));
    }

    // We're going to need a source of randomness.
    let mut rng = rand::thread_rng();

    // Handle alliteration, either by eliminating a specified
    // character, or using a random one.
    if opt_alliterate {
        // We choose the first letter from the intersection of the
        // first letters of each word list in `petnames`.
        let firsts =
            common_first_letters(&petnames.adjectives, &[&petnames.adverbs, &petnames.names]);
        // if a specific character was requested for alliteration,
        // attempt to use it.
        if let Some(c) = opt_alliterate_char {
            if firsts.contains(&c) {
                petnames.retain(|s| s.starts_with(c));
            } else {
                return Err(Error::Alliteration(
                    "no petnames begin with the choosen alliteration character".to_string(),
                ));
            }
        } else {
            // Otherwise choose the first letter at random; fails if
            // there are no letters.
            match firsts.iter().choose(&mut rng) {
                Some(c) => petnames.retain(|s| s.starts_with(*c)),
                None => {
                    return Err(Error::Alliteration(
                        "word lists have no initial letters in common".to_string(),
                    ))
                }
            };
        }
    }

    // Manage stdout.
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());

    // Warn that --count=0 is deprecated.
    if opt_count == 0 {
        eprintln!(concat!(
            "Warning: specifying --count=0 to continuously produce petnames is ",
            "deprecated and its behaviour will change in a future version; ",
            "specify --stream instead.",
        ));
    }

    // Stream if count is 0. TODO: Only stream when --stream is specified.
    let count = if opt_stream || opt_count == 0 {
        None
    } else {
        Some(opt_count)
    };

    // Get an iterator for the names we want to print out.
    if opt_non_repeating {
        printer(
            &mut writer,
            petnames.iter_non_repeating(&mut rng, opt_words, opt_separator),
            count,
        )
    } else {
        printer(
            &mut writer,
            petnames.iter(&mut rng, opt_words, opt_separator),
            count,
        )
    }
}

fn printer<OUT, NAMES>(writer: &mut OUT, names: NAMES, count: Option<usize>) -> Result<(), Error>
where
    OUT: io::Write,
    NAMES: Iterator<Item = String>,
{
    match count {
        None => {
            for name in names {
                writeln!(writer, "{}", name).map_err(suppress_disconnect)?;
            }
        }
        Some(n) => {
            for name in names.take(n) {
                writeln!(writer, "{}", name)?;
            }
        }
    }

    Ok(())
}

fn can_be_parsed<INTO>(value: String) -> Result<(), String>
where
    INTO: FromStr,
    <INTO as FromStr>::Err: std::fmt::Display,
{
    match value.parse::<INTO>() {
        Err(e) => Err(format!("{}", e)),
        Ok(_) => Ok(()),
    }
}

fn common_first_letters(init: &[&str], more: &[&[&str]]) -> HashSet<char> {
    let mut firsts = first_letters(init);
    let firsts_other: Vec<HashSet<char>> = more.iter().map(|list| first_letters(list)).collect();
    firsts.retain(|c| firsts_other.iter().all(|fs| fs.contains(c)));
    firsts
}

fn first_letters(names: &[&str]) -> HashSet<char> {
    names.iter().filter_map(|s| s.chars().next()).collect()
}

enum Words {
    Custom(String, String, String),
    Builtin,
}

impl Words {
    // Load word lists from the given directory. This function expects to find three
    // files in that directory: `adjectives.txt`, `adverbs.txt`, and `names.txt`.
    // Each should be valid UTF-8, and contain words separated by whitespace.
    fn load<T: AsRef<path::Path>>(dirname: T) -> Result<Self, Error> {
        let dirname = dirname.as_ref();
        Ok(Self::Custom(
            read_file_to_string(dirname.join("adjectives.txt"))?,
            read_file_to_string(dirname.join("adverbs.txt"))?,
            read_file_to_string(dirname.join("names.txt"))?,
        ))
    }
}

fn read_file_to_string<P: AsRef<path::Path>>(path: P) -> Result<String, Error> {
    fs::read_to_string(&path).map_err(|error| Error::FileIo(path.as_ref().to_path_buf(), error))
}

fn suppress_disconnect(err: io::Error) -> Error {
    match err.kind() {
        io::ErrorKind::BrokenPipe => Error::Disconnected,
        _ => err.into(),
    }
}
