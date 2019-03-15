//! The `twiggy` code size profiler.

#![deny(missing_docs)]
#![deny(missing_debug_implementations)]

mod demangle;
mod graph_impl;

use frozen::Frozen;
use std::cmp;
use std::collections::btree_map;
use std::collections::{BTreeMap, BTreeSet};
use std::ops;
use std::slice;
use std::u32;

/// Build up a a set of `Items`.
#[derive(Debug)]
pub struct ItemsBuilder {
    size: u32,
    size_added: u32,
    parsed: BTreeSet<Id>,
    items: BTreeMap<Id, Item>,
    edges: BTreeMap<Id, BTreeSet<Id>>,
    roots: BTreeSet<Id>,

    // Maps the offset some data begins at to its IR item's identifier, and the
    // byte length of the data.
    data: BTreeMap<u32, (Id, u32)>,
}

impl ItemsBuilder {
    /// Construct a new builder, with the given size.
    pub fn new(size: u32) -> ItemsBuilder {
        ItemsBuilder {
            size,
            size_added: 0,
            parsed: Default::default(),
            items: Default::default(),
            edges: Default::default(),
            roots: Default::default(),
            data: Default::default(),
        }
    }

    /// Add the given item to to the graph and return the `Id` that it was
    /// assigned.
    pub fn add_item(&mut self, item: Item) -> Id {
        let id = item.id;
        self.size_added += item.size;
        self.items.insert(id, item);

        let old_value = self.parsed.insert(id);
        assert!(
            old_value,
            "should not parse the same key into multiple items"
        );

        id
    }

    /// Add the given item to the graph as a root and return the `Id` that it
    /// was assigned.
    pub fn add_root(&mut self, item: Item) -> Id {
        let id = self.add_item(item);
        self.roots.insert(id);
        id
    }

    /// Add an edge between the given keys that have already been parsed into
    /// items.
    pub fn add_edge(&mut self, from: Id, to: Id) {
        debug_assert!(self.items.contains_key(&from), "`from` is not known");
        debug_assert!(self.items.contains_key(&to), "`to` is not known");

        self.edges
            .entry(from)
            .or_insert_with(BTreeSet::new)
            .insert(to);
    }

    /// Add a range of static data and the `Id` that defines it.
    pub fn link_data(&mut self, offset: i64, len: usize, id: Id) {
        if offset >= 0 && offset <= i64::from(u32::MAX) && offset as usize + len < u32::MAX as usize
        {
            self.data.insert(offset as u32, (id, len as u32));
        }
    }

    /// Locate the data section defining memory at the given offset.
    pub fn get_data(&self, offset: u32) -> Option<Id> {
        self.data
            .range(offset..)
            .next()
            .and_then(
                |(start, &(id, len))| {
                    if offset < start + len {
                        Some(id)
                    } else {
                        None
                    }
                },
            )
    }

    /// Return the size of all added items so far
    pub fn size_added(&self) -> u32 {
        self.size_added
    }

    /// Finish building the IR graph and return the resulting `Items`.
    pub fn finish(mut self) -> Items {
        let meta_root_id = Id::root();
        let meta_root = Item::new(meta_root_id, 0, Misc::new("<meta root>"));
        self.items.insert(meta_root_id, meta_root);
        self.edges.insert(meta_root_id, self.roots.clone());

        Items {
            size: self.size,
            dominator_tree: None,
            retained_sizes: None,
            predecessors: None,
            immediate_dominators: None,
            items: Frozen::freeze(self.items),
            edges: Frozen::freeze(
                self.edges
                    .into_iter()
                    .map(|(from, tos)| (from, tos.into_iter().collect::<Vec<_>>()))
                    .collect(),
            ),
            roots: Frozen::freeze(self.roots),
            meta_root: meta_root_id,
        }
    }
}

/// The architecture- and target-independent internal representation of
/// functions, sections, etc in a file that is being size profiled.
///
/// Constructed with `ItemsBuilder`.
#[derive(Debug)]
pub struct Items {
    size: u32,
    dominator_tree: Option<BTreeMap<Id, Vec<Id>>>,
    immediate_dominators: Option<BTreeMap<Id, Id>>,
    retained_sizes: Option<BTreeMap<Id, u32>>,
    predecessors: Option<BTreeMap<Id, Vec<Id>>>,
    items: Frozen<BTreeMap<Id, Item>>,
    edges: Frozen<BTreeMap<Id, Vec<Id>>>,
    roots: Frozen<BTreeSet<Id>>,
    meta_root: Id,
}

impl ops::Index<Id> for Items {
    type Output = Item;

    fn index(&self, id: Id) -> &Item {
        &self.items[&id]
    }
}

impl Items {
    /// Iterate over all of the IR items.
    pub fn iter(&self) -> Iter {
        Iter {
            inner: self.items.iter(),
        }
    }

    /// Iterate over an item's neighbors.
    pub fn neighbors(&self, id: Id) -> Neighbors {
        Neighbors {
            inner: self
                .edges
                .get(&id)
                .map_or_else(|| [].iter(), |edges| edges.iter()),
        }
    }

    /// Iterate over an item's predecessors.
    pub fn predecessors(&self, id: Id) -> Predecessors {
        Predecessors {
            inner: self
                .predecessors
                .as_ref()
                .expect("To access predecessors, must have already called compute_predecessors")
                .get(&id)
                .map_or_else(|| [].iter(), |edges| edges.iter()),
        }
    }

    /// The size of the total binary, containing all items.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Get the id of the "meta root" which is a single root item with edges to
    /// all of the real roots.
    pub fn meta_root(&self) -> Id {
        self.meta_root
    }

    /// Force computation of predecessors.
    pub fn compute_predecessors(&mut self) {
        if self.predecessors.is_some() {
            return;
        }

        let mut predecessors = BTreeMap::new();

        for (from, tos) in self.edges.iter() {
            for to in tos {
                predecessors
                    .entry(*to)
                    .or_insert_with(BTreeSet::new)
                    .insert(*from);
            }
        }

        self.predecessors = Some(
            predecessors
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().collect()))
                .collect(),
        );
    }

    /// Compute dominators for each item.
    pub fn compute_dominators(&mut self) {
        if self.immediate_dominators.is_some() {
            return;
        }

        let mut immediate_dominators = BTreeMap::new();
        let dominators = petgraph::algo::dominators::simple_fast(&*self, self.meta_root);

        for item in self.iter() {
            if let Some(idom) = dominators.immediate_dominator(item.id()) {
                immediate_dominators.insert(item.id(), idom);
            }
        }

        self.immediate_dominators = Some(immediate_dominators);
    }

    /// Get a refercence to immediate dominators
    pub fn immediate_dominators(&self) -> &BTreeMap<Id, Id> {
        self.immediate_dominators
            .as_ref()
            .expect("must call compute_immediate_dominators before calling immediate_dominators")
    }

    /// Force computation of the dominator tree.
    pub fn compute_dominator_tree(&mut self) {
        if self.dominator_tree.is_some() {
            return;
        }

        let mut dominator_tree = BTreeMap::new();
        let dominators = petgraph::algo::dominators::simple_fast(&*self, self.meta_root);
        for item in self.iter() {
            if let Some(idom) = dominators.immediate_dominator(item.id()) {
                dominator_tree
                    .entry(idom)
                    .or_insert_with(BTreeSet::new)
                    .insert(item.id());
            }
        }

        self.dominator_tree = Some(
            dominator_tree
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().collect()))
                .collect(),
        );
    }

    /// Get a reference to the dominator tree.
    ///
    /// Must have already called `compute_dominator_tree`.
    pub fn dominator_tree(&self) -> &BTreeMap<Id, Vec<Id>> {
        self.dominator_tree
            .as_ref()
            .expect("must call compute_dominator_tree before calling dominator_tree")
    }

    /// Force computation of the retained sizes of each IR item.
    pub fn compute_retained_sizes(&mut self) {
        if self.retained_sizes.is_some() {
            return;
        }
        self.compute_dominator_tree();

        fn recursive_retained_size(
            retained_sizes: &mut BTreeMap<Id, u32>,
            items: &Items,
            item: &Item,
            dominator_tree: &BTreeMap<Id, Vec<Id>>,
        ) -> u32 {
            // Although the dominator tree cannot have cycles, because we
            // compute retained sizes in item iteration order, rather than from
            // the bottom of the dominator tree up, it is possible we have
            // already computed the retained sizes for subtrees.
            if let Some(rsize) = retained_sizes.get(&item.id()) {
                return *rsize;
            }

            let mut rsize = item.size();
            if let Some(children) = dominator_tree.get(&item.id()) {
                for child in children {
                    rsize += recursive_retained_size(
                        retained_sizes,
                        items,
                        &items[*child],
                        dominator_tree,
                    );
                }
            }

            let old_value = retained_sizes.insert(item.id(), rsize);
            // The dominator tree is a proper tree, so there shouldn't be
            // any cycles.
            assert!(old_value.is_none());
            rsize
        }

        let mut retained_sizes = BTreeMap::new();
        {
            let dominator_tree = self.dominator_tree.as_ref().unwrap();
            for item in self.iter() {
                recursive_retained_size(&mut retained_sizes, self, item, dominator_tree);
            }
        }
        self.retained_sizes = Some(retained_sizes);
    }

    /// Get the given item's retained size.
    pub fn retained_size(&self, id: Id) -> u32 {
        self.retained_sizes
            .as_ref()
            .expect(
                "Cannot call retained_sizes unless compute_retained_sizes \
                 has already been called",
            )
            .get(&id)
            .cloned()
            .unwrap()
    }

    /// Get an item with the given name.
    pub fn get_item_by_name(&self, name: &str) -> Option<&Item> {
        for item in self.iter() {
            if item.name() == name {
                return Some(item);
            }
        }

        None // Return `None` if `name` did not match any items.
    }
}

/// An iterator over an item's neighbors.
#[derive(Debug)]
pub struct Neighbors<'a> {
    inner: slice::Iter<'a, Id>,
}

impl<'a> Iterator for Neighbors<'a> {
    type Item = Id;

    #[inline]
    fn next(&mut self) -> Option<Id> {
        self.inner.next().cloned()
    }
}

/// An iterator over an item's predecessors.
#[derive(Debug)]
pub struct Predecessors<'a> {
    inner: slice::Iter<'a, Id>,
}

impl<'a> Iterator for Predecessors<'a> {
    type Item = Id;

    #[inline]
    fn next(&mut self) -> Option<Id> {
        self.inner.next().cloned()
    }
}

/// An iterator over IR items. Created by `Items::iter`.
#[derive(Clone, Debug)]
pub struct Iter<'a> {
    inner: btree_map::Iter<'a, Id, Item>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(_, item)| item)
    }
}

/// An item's unique identifier.
/// (section index, item within that section index)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u32, u32);

impl Id {
    /// Create an `Id` for a the given section.
    pub fn section(section: usize) -> Id {
        assert!(section < u32::MAX as usize);
        Id(section as u32, u32::MAX)
    }

    /// Create an `Id` for a given entry in a given section.
    pub fn entry(section: usize, index: usize) -> Id {
        assert!(section < u32::MAX as usize);
        assert!(index < u32::MAX as usize);
        Id(section as u32, index as u32)
    }

    /// Create the `Id` for the "meta root".
    pub fn root() -> Id {
        Id(u32::MAX, u32::MAX)
    }

    /// Get the real id of a item.
    pub fn serializable(self) -> u64 {
        let top = (u64::from(self.0)) << 32;
        top | u64::from(self.1)
    }
}

/// An item in the binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    id: Id,
    size: u32,
    kind: ItemKind,
}

impl Item {
    /// Construct a new `Item` of the given kind.
    pub fn new<K>(id: Id, size: u32, kind: K) -> Item
    where
        K: Into<ItemKind>,
    {
        Item {
            id,
            size,
            kind: kind.into(),
        }
    }

    /// Get this item's identifier.
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Get this item's size.
    #[inline]
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Get this item's name.
    pub fn name(&self) -> &str {
        match &self.kind {
            ItemKind::Code(code) => code
                .demangled()
                .or_else(|| code.name())
                .unwrap_or_else(|| code.annotation()),
            ItemKind::Data(Data { name, .. }) => name,
            ItemKind::Func(func) => func.annotation(),
            ItemKind::Debug(DebugInfo { name, .. }) => name,
            ItemKind::Misc(Misc { name, .. }) => name,
        }
    }

    /// Get this item's annotated name.
    pub fn annotated_name(&self) -> String {
        match &self.kind {
            ItemKind::Code(code) => {
                if let Some(name) = code.demangled().or_else(|| code.name()) {
                    format!("{}: {}", code.annotation(), name)
                } else {
                    code.annotation().to_string()
                }
            }
            ItemKind::Data(Data { name, .. }) => name.to_string(),
            ItemKind::Func(func) => {
                if let Some(name) = func.demangled().or_else(|| func.name()) {
                    format!("{}: {}", func.annotation(), name)
                } else {
                    func.annotation().to_string()
                }
            }
            ItemKind::Debug(DebugInfo { name, .. }) => name.to_string(),
            ItemKind::Misc(Misc { name, .. }) => name.to_string(),
        }
    }

    /// Get this item's kind.
    #[inline]
    pub fn kind(&self) -> &ItemKind {
        &self.kind
    }

    /// The the name of the generic function that this is a monomorphization of
    /// (if any).
    #[inline]
    pub fn monomorphization_of(&self) -> Option<&str> {
        if let ItemKind::Code(ref code) = self.kind {
            code.monomorphization_of()
        } else {
            None
        }
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, rhs: &Item) -> Option<cmp::Ordering> {
        self.id.partial_cmp(&rhs.id)
    }
}

impl Ord for Item {
    fn cmp(&self, rhs: &Item) -> cmp::Ordering {
        self.id.cmp(&rhs.id)
    }
}

/// The kind of item in the binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// Executable code. Function bodies.
    Code(Code),

    /// Data inside the binary that may or may not end up loaded into memory
    /// with the executable code.
    Data(Data),

    /// Function definition. Declares the signature of a function.
    Func(Function),

    /// Debugging symbols and information, such as a DWARF section.
    Debug(DebugInfo),

    /// Miscellaneous item. Perhaps metadata. Perhaps something else.
    Misc(Misc),
}

impl ItemKind {
    /// Returns true if `self` is the `Data` variant
    pub fn is_data(&self) -> bool {
        match self {
            ItemKind::Data(_) => true,
            _ => false,
        }
    }
}

impl From<Code> for ItemKind {
    fn from(c: Code) -> ItemKind {
        ItemKind::Code(c)
    }
}

impl From<Data> for ItemKind {
    fn from(d: Data) -> ItemKind {
        ItemKind::Data(d)
    }
}

impl From<Function> for ItemKind {
    fn from(f: Function) -> ItemKind {
        ItemKind::Func(f)
    }
}

impl From<DebugInfo> for ItemKind {
    fn from(d: DebugInfo) -> ItemKind {
        ItemKind::Debug(d)
    }
}

impl From<Misc> for ItemKind {
    fn from(m: Misc) -> ItemKind {
        ItemKind::Misc(m)
    }
}

/// Executable code. Function bodies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    name: Option<String>,
    annotation: String,
    demangled: Option<String>,
    monomorphization_of: Option<String>,
}

impl Code {
    /// Construct a new IR item for executable code.
    pub fn new(name: Option<String>, annotation: String) -> Code {
        let demangled = name.as_ref().and_then(|n| demangle::demangle(&n));
        let monomorphization_of = demangled
            .as_ref()
            .and_then(|d| Self::extract_generic_function(&d));
        Code {
            name,
            annotation,
            demangled,
            monomorphization_of,
        }
    }

    /// Get the name of this function body, if any.
    pub(crate) fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_str())
    }

    /// Get the annotation for this function body, if any.
    pub(crate) fn annotation(&self) -> &str {
        self.annotation.as_str()
    }

    /// Get the demangled name of this function, if any.
    pub fn demangled(&self) -> Option<&str> {
        self.demangled.as_ref().map(|s| s.as_str())
    }

    /// Get the name of the generic function that this is a monomorphization of,
    /// if any.
    pub fn monomorphization_of(&self) -> Option<&str> {
        self.monomorphization_of.as_ref().map(|s| s.as_str())
    }

    fn extract_generic_function(demangled: &str) -> Option<String> {
        // XXX: This is some hacky, ad-hoc parsing shit! This should
        // approximately work for Rust and C++ symbols, but who knows for other
        // languages. Also, it almost definitely has bugs!

        // First, check for Rust-style symbols by looking for Rust's
        // "::h1234567890" hash from the end of the symbol. If it's there, the
        // generic function is just the symbol without that hash, so remove it.
        //
        // I know what you're thinking, and it's true: mangled (and therefore
        // also demangled) Rust symbols don't include the concrete type(s) used
        // to instantiate the generic function, which gives us much less to work
        // with than we have with C++ demangled symbols. It would sure be nice
        // if we could tell the user more about the monomorphization, but
        // alas... :(
        if let Some(idx) = demangled.rfind("::h") {
            let idx2 = demangled.rfind("::").unwrap();
            assert!(idx2 >= idx);
            if idx2 == idx {
                let generic = demangled[..idx].to_string();
                return Some(generic);
            }
        }

        // From here on out, we assume we are dealing with C++ symbols.
        //
        // Find the '<' and '>' that hug the generic type(s).
        let open_bracket = match demangled.char_indices().find(|&(_, ch)| ch == '<') {
            None => return None,
            Some((idx, _)) => idx,
        };
        let close_bracket = match demangled.char_indices().rev().find(|&(_, ch)| ch == '>') {
            None => return None,
            Some((idx, _)) => idx,
        };

        // If the '<' doesn't come before the '>', then we aren't looking at a
        // generic function instantiation. If there isn't anything proceeding
        // the '<', then we aren't looking at a generic function instantiation
        // (most likely looking at a Rust trait method's implementation, like
        // `<MyType as SomeTrait>::trait_method()`).
        if close_bracket < open_bracket || open_bracket == 0 {
            return None;
        }

        // And now we say that the generic function is the thing proceeding the
        // '<'. Good enough!
        Some(demangled[..open_bracket].to_string())
    }
}

/// Data inside the binary that may or may not end up loaded into memory
/// with the executable code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data {
    name: String,
    ty: Option<String>,
}

impl Data {
    /// Construct a new `Data` that has a type of the given type name, if known.
    pub fn new(name: &str, ty: Option<String>) -> Data {
        Data {
            name: name.to_string(),
            ty,
        }
    }
}

/// Function definition. Declares the signature of a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    name: Option<String>,
    annotation: String,
    demangled: Option<String>,
}

impl Function {
    /// Construct a new IR item for function definition.
    pub fn new(name: Option<String>, annotation: String) -> Function {
        let demangled = name.as_ref().and_then(|n| demangle::demangle(&n));
        Function {
            name,
            annotation,
            demangled,
        }
    }

    /// Get the name of this function, if any.
    pub(crate) fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_str())
    }

    /// Get the annotation for this function, if any.
    pub(crate) fn annotation(&self) -> &str {
        self.annotation.as_ref()
    }

    /// Get the demangled name of this function, if any.
    pub fn demangled(&self) -> Option<&str> {
        self.demangled.as_ref().map(|s| s.as_str())
    }
}

/// Debugging symbols and information, such as DWARF sections.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DebugInfo {
    name: String,
}

impl DebugInfo {
    /// Construct a new IR item for debug information and symbols.
    pub fn new(name: &str) -> DebugInfo {
        DebugInfo {
            name: name.to_string(),
        }
    }
}

/// Miscellaneous item. Perhaps metadata. Perhaps something else.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Misc {
    name: String,
}

impl Misc {
    /// Construct a new miscellaneous IR item.
    pub fn new(name: &str) -> Misc {
        Misc {
            name: name.to_string(),
        }
    }
}
