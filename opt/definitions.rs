// Fun times ahead!
//
// Apparently, proc-macros don't play well with `cfg_attr` yet, and their
// combination is buggy. So we can't use cfg_attr to choose between
// `wasm-bindgen` and `structopt` depending on if we're building the CLI or the
// wasm API respectively. Instead, we have `build.rs` remove unwanted attributes
// for us by invoking `grep`.
//
// It's terrible! But it works for now.

use structopt::StructOpt;

/// Options for configuring `twiggy`.
#[derive(Clone, Debug)]
#[derive(StructOpt)]
#[structopt(
    about = "\n`twiggy` is a code size profiler.\n\nIt analyzes a binary's call graph to answer questions like:\n\n* Why was this function included in the binary in the first place?\n\n* What is the retained size of this function? I.e. how much space\n  would be saved if I removed it and all the functions that become\n  dead code after its removal.\n\nUse `twiggy` to make your binaries slim!"
)]
pub enum Options {
    /// List the top code size offenders in a binary.
    #[structopt(name = "top")]
    Top(Top),

    /// Compute and display the dominator tree for a binary's call graph.
    #[structopt(name = "dominators")]
    Dominators(Dominators),

    /// Find and display the call paths to a function in the given binary's call
    /// graph.
    #[structopt(name = "paths")]
    Paths(Paths),

    /// List the generic function monomorphizations that are contributing to
    /// code bloat.
    #[structopt(name = "monos")]
    Monos(Monos),

    /// Diff the old and new versions of a binary to see what sizes changed.
    #[structopt(name = "diff")]
    Diff(Diff),

    /// Find and display code and data that is not transitively referenced by
    /// any exports or public functions.
    #[structopt(name = "garbage")]
    Garbage(Garbage),
}

/// List the top code size offenders in a binary.
#[wasm_bindgen]
#[derive(Clone, Debug)]
#[derive(StructOpt)]
pub struct Top {
    /// The path to the input binary to size profile.
    #[cfg(feature = "cli")]
    #[structopt(parse(from_os_str))]
    input: path::PathBuf,

    /// The parse mode for the input binary data.
    #[cfg(feature = "cli")]
    #[structopt(long = "mode", default_value = "auto")]
    parse_mode: traits::ParseMode,

    /// The destination to write the output to. Defaults to `stdout`.
    #[cfg(feature = "cli")]
    #[structopt(short = "o", default_value = "-")]
    output_destination: OutputDestination,

    /// The format the output should be written in.
    #[cfg(feature = "cli")]
    #[structopt(short = "f", long = "format", default_value = "text")]
    output_format: traits::OutputFormat,

    /// The maximum number of items to display.
    #[structopt(short = "n", default_value = "4294967295")]
    max_items: u32,

    /// Display retaining paths.
    #[structopt(short = "r", long = "retaining-paths")]
    retaining_paths: bool,

    /// Sort list by retained size, rather than shallow size.
    #[structopt(long = "retained")]
    retained: bool,
}

impl Default for Top {
    fn default() -> Top {
        Top {
            #[cfg(feature = "cli")]
            input: Default::default(),
            #[cfg(feature = "cli")]
            parse_mode: Default::default(),
            #[cfg(feature = "cli")]
            output_destination: Default::default(),
            #[cfg(feature = "cli")]
            output_format: Default::default(),

            max_items: 4_294_967_295,
            retaining_paths: false,
            retained: false,
        }
    }
}

#[wasm_bindgen]
impl Top {
    /// Construct a new, default `Top`.
    pub fn new() -> Top {
        Top::default()
    }

    /// The maximum number of items to display.
    pub fn max_items(&self) -> u32 {
        self.max_items
    }

    /// Display retaining paths.
    pub fn retaining_paths(&self) -> bool {
        self.retaining_paths
    }

    /// Sort list by retained size, rather than shallow size.
    pub fn retained(&self) -> bool {
        self.retained
    }

    /// Set the maximum number of items to display.
    pub fn set_max_items(&mut self, n: u32) {
        self.max_items = n;
    }

    /// Set whether to display and compute retaining paths.
    pub fn set_retaining_paths(&mut self, do_it: bool) {
        self.retaining_paths = do_it;
    }

    /// Set whether to sort list by retained size, rather than shallow size.
    pub fn set_retained(&mut self, do_it: bool) {
        self.retained = do_it;
    }
}

/// Compute and display the dominator tree for a binary's call graph.
#[wasm_bindgen]
#[derive(Clone, Debug, Default)]
#[derive(StructOpt)]
pub struct Dominators {
    /// The path to the input binary to size profile.
    #[cfg(feature = "cli")]
    #[structopt(parse(from_os_str))]
    input: path::PathBuf,

    /// The parse mode for the input binary data.
    #[cfg(feature = "cli")]
    #[structopt(long = "mode", default_value = "auto")]
    parse_mode: traits::ParseMode,

    /// The destination to write the output to. Defaults to `stdout`.
    #[cfg(feature = "cli")]
    #[structopt(short = "o", default_value = "-")]
    output_destination: OutputDestination,

    /// The format the output should be written in.
    #[cfg(feature = "cli")]
    #[structopt(short = "f", long = "format", default_value = "text")]
    output_format: traits::OutputFormat,

    /// The name of the function whose dominator subtree should be printed.
    items: Vec<String>,

    /// The maximum depth to print the dominators tree.
    #[structopt(short = "d")]
    max_depth: Option<u32>,

    /// The maximum number of rows, regardless of depth in the tree, to display.
    #[structopt(short = "r")]
    max_rows: Option<u32>,

    /// Whether or not `items` should be treated as regular expressions.
    #[structopt(long = "regex")]
    using_regexps: bool,
}

impl Dominators {
    // TODO: wasm-bindgen does not support sending Vec<String> across
    // the wasm ABI boundary yet.

    /// The items whose dominators subtree should be printed.
    pub fn items(&self) -> &[String] {
        &self.items
    }
}

#[wasm_bindgen]
impl Dominators {
    /// Construct a new, default `Dominators`.
    pub fn new() -> Dominators {
        Dominators::default()
    }

    /// The maximum depth to print the dominators tree.
    pub fn max_depth(&self) -> u32 {
        self.max_depth.unwrap_or(u32::MAX)
    }

    /// The maximum number of rows, regardless of depth in the tree, to display.
    pub fn max_rows(&self) -> u32 {
        self.max_rows.unwrap_or(u32::MAX)
    }

    /// Whether or not `items` should be treated as regular expressions.
    pub fn using_regexps(&self) -> bool {
        self.using_regexps
    }

    /// Set the maximum depth to print the dominators tree.
    pub fn set_max_depth(&mut self, max_depth: u32) {
        self.max_depth = Some(max_depth);
    }

    /// Set the maximum number of rows, regardless of depth in the tree, to display.
    pub fn set_max_rows(&mut self, max_rows: u32) {
        self.max_rows = Some(max_rows);
    }

    /// Set whether or not `items` should be treated as regular expressions.
    pub fn set_using_regexps(&mut self, using_regexps: bool) {
        self.using_regexps = using_regexps;
    }
}

/// Find and display the call paths to a function in the given binary's call
/// graph.
#[wasm_bindgen]
#[derive(Clone, Debug)]
#[derive(StructOpt)]
pub struct Paths {
    /// The path to the input binary to size profile.
    #[cfg(feature = "cli")]
    #[structopt(parse(from_os_str))]
    input: path::PathBuf,

    /// The parse mode for the input binary data.
    #[cfg(feature = "cli")]
    #[structopt(long = "mode", default_value = "auto")]
    parse_mode: traits::ParseMode,

    /// The destination to write the output to. Defaults to `stdout`.
    #[cfg(feature = "cli")]
    #[structopt(short = "o", default_value = "-")]
    output_destination: OutputDestination,

    /// The format the output should be written in.
    #[cfg(feature = "cli")]
    #[structopt(short = "f", long = "format", default_value = "text")]
    output_format: traits::OutputFormat,

    /// The functions to find call paths to.
    functions: Vec<String>,

    /// The maximum depth to print the paths.
    #[structopt(short = "d", default_value = "10")]
    max_depth: u32,

    /// The maximum number of paths, regardless of depth in the tree, to display.
    #[structopt(short = "r", default_value = "10")]
    max_paths: u32,

    /// This direction of the path traversal.
    #[structopt(long = "descending")]
    descending: bool,

    /// Whether or not `functions` should be treated as regular expressions.
    #[structopt(long = "regex")]
    using_regexps: bool,
}

impl Default for Paths {
    fn default() -> Paths {
        Paths {
            #[cfg(feature = "cli")]
            input: Default::default(),
            #[cfg(feature = "cli")]
            parse_mode: Default::default(),
            #[cfg(feature = "cli")]
            output_destination: Default::default(),
            #[cfg(feature = "cli")]
            output_format: Default::default(),

            functions: Default::default(),
            max_depth: 10,
            max_paths: 10,
            descending: false,
            using_regexps: false,
        }
    }
}

impl Paths {
    // TODO: wasm-bindgen doesn't support sending Vec<String> across the wasm
    // ABI boundary yet.

    /// The functions to find call paths to.
    pub fn functions(&self) -> &[String] {
        &self.functions
    }
}

#[wasm_bindgen]
impl Paths {
    /// Construct a new, default `Paths`.
    pub fn new() -> Paths {
        Paths::default()
    }

    /// Add a function to find call paths for.
    pub fn add_function(&mut self, function: String) {
        self.functions.push(function);
    }

    /// The maximum depth to print the paths.
    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }

    /// The maximum number of paths, regardless of depth in the tree, to display.
    pub fn max_paths(&self) -> u32 {
        self.max_paths
    }

    /// The direction in which the call paths are traversed.
    pub fn descending(&self) -> bool {
        self.descending
    }

    /// Whether or not `functions` should be treated as regular expressions.
    pub fn using_regexps(&self) -> bool {
        self.using_regexps
    }

    /// Set the maximum depth to print the paths.
    pub fn set_max_depth(&mut self, max_depth: u32) {
        self.max_depth = max_depth;
    }

    /// Set the maximum number of paths, regardless of depth in the tree, to display.
    pub fn set_max_paths(&mut self, max_paths: u32) {
        self.max_paths = max_paths;
    }

    /// Set the call path traversal direction.
    pub fn set_descending(&mut self, descending: bool) {
        self.descending = descending;
    }

    /// Set Whether or not `functions` should be treated as regular expressions.
    pub fn set_using_regexps(&mut self, using_regexps: bool) {
        self.using_regexps = using_regexps;
    }
}

/// List the generic function monomorphizations that are contributing to
/// code bloat.
#[wasm_bindgen]
#[derive(Clone, Debug)]
#[derive(StructOpt)]
pub struct Monos {
    /// The path to the input binary to size profile.
    #[cfg(feature = "cli")]
    #[structopt(parse(from_os_str))]
    input: path::PathBuf,

    /// The names of the generic functions whose monomorphizations
    /// should be printed.
    functions: Vec<String>,

    /// The parse mode for the input binary data.
    #[cfg(feature = "cli")]
    #[structopt(short = "d", long = "mode", default_value = "auto")]
    parse_mode: traits::ParseMode,

    /// The destination to write the output to. Defaults to `stdout`.
    #[cfg(feature = "cli")]
    #[structopt(short = "o", default_value = "-")]
    output_destination: OutputDestination,

    /// The format the output should be written in.
    #[cfg(feature = "cli")]
    #[structopt(short = "f", long = "format", default_value = "text")]
    output_format: traits::OutputFormat,

    /// Hide individual monomorphizations and only show the generic functions.
    #[structopt(short = "g", long = "only-generics")]
    only_generics: bool,

    /// The maximum number of generics to list.
    #[structopt(short = "m", long = "max-generics", default_value = "10")]
    max_generics: u32,

    /// The maximum number of individual monomorphizations to list for each
    /// listed generic function.
    #[structopt(short = "n", long = "max-monos", default_value = "10")]
    max_monos: u32,

    /// List all generics and all of their individual monomorphizations.
    /// If combined with -g then monomorphizations are hidden.
    /// Overrides -m <max_generics> and -n <max_monos>
    #[structopt(short = "a", long = "all")]
    all_generics_and_monos: bool,

    /// List all generics. Overrides -m <max_generics>
    #[structopt(long = "all-generics")]
    all_generics: bool,

    /// List all individual monomorphizations for each listed generic
    /// function. Overrides -n <max_monos>
    #[structopt(long = "all-monos")]
    all_monos: bool,

    /// Whether or not `names` should be treated as regular expressions.
    #[structopt(long = "regex")]
    using_regexps: bool,
}

impl Default for Monos {
    fn default() -> Monos {
        Monos {
            #[cfg(feature = "cli")]
            input: Default::default(),
            #[cfg(feature = "cli")]
            parse_mode: Default::default(),
            #[cfg(feature = "cli")]
            output_destination: Default::default(),
            #[cfg(feature = "cli")]
            output_format: Default::default(),

            functions: Default::default(),

            only_generics: false,
            max_generics: 10,
            max_monos: 10,

            all_generics_and_monos: false,
            all_generics: false,
            all_monos: false,

            using_regexps: false,
        }
    }
}

impl Monos {
    // TODO: wasm-bindgen doesn't support sending Vec<String> across the wasm
    // ABI boundary yet.

    /// The functions to find call paths to.
    pub fn functions(&self) -> &[String] {
        &self.functions
    }
}

#[wasm_bindgen]
impl Monos {
    /// Construct a new, default `Monos`.
    pub fn new() -> Monos {
        Monos::default()
    }

    /// Hide individual monomorphizations and only show the generic functions.
    pub fn only_generics(&self) -> bool {
        self.only_generics
    }

    /// The maximum number of generics to list.
    pub fn max_generics(&self) -> u32 {
        if self.all_generics_and_monos || self.all_generics {
            u32::MAX
        } else {
            self.max_generics
        }
    }

    /// The maximum number of individual monomorphizations to list for each
    /// generic function.
    pub fn max_monos(&self) -> u32 {
        if self.all_generics_and_monos || self.all_monos {
            u32::MAX
        } else {
            self.max_monos
        }
    }

    /// Whether or not `functions` should be treated as regular expressions.
    pub fn using_regexps(&self) -> bool {
        self.using_regexps
    }

    /// Set whether to hide individual monomorphizations and only show the
    /// generic functions.
    pub fn set_only_generics(&mut self, do_it: bool) {
        self.only_generics = do_it;
    }

    /// Set the maximum number of generics to list.
    pub fn set_max_generics(&mut self, max: u32) {
        self.max_generics = max;
        self.all_generics = false;
        if self.all_generics_and_monos {
            self.all_generics_and_monos = false;
            self.all_monos = true;
        }
    }

    /// Set the maximum number of individual monomorphizations to list for each
    /// generic function.
    pub fn set_max_monos(&mut self, max: u32) {
        self.max_monos = max;
        self.all_monos = false;
        if self.all_generics_and_monos {
            self.all_generics_and_monos = false;
            self.all_generics = true;
        }
    }
}

/// Diff the old and new versions of a binary to see what sizes changed.
#[wasm_bindgen]
#[derive(Clone, Debug)]
#[derive(StructOpt)]
pub struct Diff {
    /// The path to the old version of the input binary.
    #[cfg(feature = "cli")]
    #[structopt(parse(from_os_str))]
    old_input: path::PathBuf,

    /// The parse mode for the input binary data.
    #[cfg(feature = "cli")]
    #[structopt(long = "mode", default_value = "auto")]
    parse_mode: traits::ParseMode,

    /// The path to the new version of the input binary.
    #[cfg(feature = "cli")]
    #[structopt(parse(from_os_str))]
    new_input: path::PathBuf,

    /// The destination to write the output to. Defaults to `stdout`.
    #[cfg(feature = "cli")]
    #[structopt(short = "o", default_value = "-")]
    output_destination: OutputDestination,

    /// The format the output should be written in.
    #[cfg(feature = "cli")]
    #[structopt(short = "f", long = "format", default_value = "text")]
    output_format: traits::OutputFormat,

    /// The name of the item(s) whose diff should be printed.
    items: Vec<String>,

    /// The maximum number of items to display.
    #[structopt(short = "n", default_value = "20")]
    max_items: u32,

    /// Whether or not `items` should be treated as regular expressions.
    #[structopt(long = "regex")]
    using_regexps: bool,

    /// Displays all items. Overrides -n <max_items>
    #[structopt(short = "a", long = "all")]
    all_items: bool,
}

impl Default for Diff {
    fn default() -> Diff {
        Diff {
            #[cfg(feature = "cli")]
            old_input: Default::default(),
            #[cfg(feature = "cli")]
            parse_mode: Default::default(),
            #[cfg(feature = "cli")]
            new_input: Default::default(),
            #[cfg(feature = "cli")]
            output_destination: Default::default(),
            #[cfg(feature = "cli")]
            output_format: Default::default(),

            items: Default::default(),
            max_items: 20,
            using_regexps: false,
            all_items: false,
        }
    }
}

impl Diff {
    // TODO: wasm-bindgen does not support sending Vec<String> across
    // the wasm ABI boundary yet.

    /// The items whose dominators subtree should be printed.
    pub fn items(&self) -> &[String] {
        &self.items
    }
}

#[wasm_bindgen]
impl Diff {
    /// The maximum number of items to display.
    pub fn max_items(&self) -> u32 {
        if self.all_items {
            u32::MAX
        } else {
            self.max_items
        }
    }

    /// Whether or not `items` should be treated as regular expressions.
    pub fn using_regexps(&self) -> bool {
        self.using_regexps
    }

    /// Set the maximum number of items to display.
    pub fn set_max_items(&mut self, n: u32) {
        self.max_items = n;
        self.all_items = false;
    }

    /// Set whether or not `items` should be treated as regular expressions.
    pub fn set_using_regexps(&mut self, using_regexps: bool) {
        self.using_regexps = using_regexps;
    }
}

/// Find and display code and data that is not transitively referenced by any
/// exports or public functions.
#[wasm_bindgen]
#[derive(Clone, Debug)]
#[derive(StructOpt)]
pub struct Garbage {
    /// The path to the input binary to size profile.
    #[cfg(feature = "cli")]
    #[structopt(parse(from_os_str))]
    input: path::PathBuf,

    /// The parse mode for the input binary data.
    #[cfg(feature = "cli")]
    #[structopt(long = "mode", default_value = "auto")]
    parse_mode: traits::ParseMode,

    /// The destination to write the output to. Defaults to `stdout`.
    #[cfg(feature = "cli")]
    #[structopt(short = "o", default_value = "-")]
    output_destination: OutputDestination,

    /// The format the output should be written in.
    #[cfg(feature = "cli")]
    #[structopt(short = "f", long = "format", default_value = "text")]
    output_format: traits::OutputFormat,

    /// The maximum number of items to display.
    #[structopt(short = "n", default_value = "10")]
    max_items: u32,

    /// Display all items. Overrides -n <max_items>
    #[structopt(short = "a", long = "all")]
    all_items: bool,

    /// Show data segments rather than summarizing them in a single line.
    #[structopt(long = "show-data-segments")]
    show_data_segments: bool,
}

impl Default for Garbage {
    fn default() -> Garbage {
        Garbage {
            #[cfg(feature = "cli")]
            input: Default::default(),
            #[cfg(feature = "cli")]
            parse_mode: Default::default(),
            #[cfg(feature = "cli")]
            output_destination: Default::default(),
            #[cfg(feature = "cli")]
            output_format: Default::default(),

            max_items: 10,
            all_items: false,
            show_data_segments: false,
        }
    }
}

#[wasm_bindgen]
impl Garbage {
    /// Construct a new, default `Garbage`
    pub fn new() -> Garbage {
        Garbage::default()
    }

    /// The maximum number of items to display.
    pub fn max_items(&self) -> u32 {
        if self.all_items {
            u32::MAX
        } else {
            self.max_items
        }
    }

    /// Set the maximum number of items to display.
    pub fn set_max_items(&mut self, max: u32) {
        self.max_items = max;
        self.all_items = false;
    }

    /// Should data segments be shown normally or summarized in a single line?
    pub fn show_data_segments(&self) -> bool {
        self.show_data_segments
    }
}
