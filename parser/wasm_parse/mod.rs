use super::Parse;
use std::collections::HashMap;
use twiggy_ir::{self as ir, Id};
use twiggy_traits as traits;
use wasmparser::SectionWithLimitedItems;
use wasmparser::{self, Operator, SectionReader, Type};

#[derive(Default)]
pub struct SectionIndices {
    type_: Option<usize>,
    code: Option<usize>,
    functions: Vec<Id>,
    tables: Vec<Id>,
    memories: Vec<Id>,
    globals: Vec<Id>,
}

struct IndexedSection<'a>(usize, wasmparser::Payload<'a>);

struct CodeSection<'a> {
    index: usize,
    reader: wasmparser::CodeSectionReader<'a>,
    byte_size: usize,
}

struct FunctionSection<'a> {
    index: usize,
    reader: wasmparser::FunctionSectionReader<'a>,
    byte_size: usize,
}

pub struct ModuleReader<'a> {
    data: &'a [u8],
    offset: usize,
    parser: wasmparser::Parser,
}

impl<'a> ModuleReader<'a> {
    pub fn new(data: &[u8]) -> ModuleReader {
        ModuleReader {
            data: data,
            offset: 0,
            parser: wasmparser::Parser::new(0),
        }
    }

    fn current_position(&self) -> usize {
        self.offset
    }

    fn eof(&self) -> bool {
        self.offset == self.data.len()
    }

    fn read(&mut self) -> Result<wasmparser::Payload<'a>, traits::Error> {
        let (section, bytes_consumed) =
            match self.parser.parse(&self.data[self.offset..], self.eof())? {
                wasmparser::Chunk::NeedMoreData { .. } => {
                    panic!("@@@ nyi");
                }
                wasmparser::Chunk::Parsed { consumed, payload } => (payload, consumed),
            };
        self.offset += bytes_consumed;
        Ok(section)
    }

    fn new_code_section(
        &self,
        index: usize,
        start_offset: usize,
        byte_range: wasmparser::Range,
    ) -> Result<CodeSection<'a>, traits::Error> {
        Ok(CodeSection {
            index: index,
            reader: wasmparser::CodeSectionReader::new(
                &self.data[byte_range.start..byte_range.end],
                byte_range.start,
            )?,
            byte_size: byte_range.end - start_offset,
        })
    }
}

impl<'a> Parse<'a> for ModuleReader<'a> {
    type ItemsExtra = ();

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        _extra: (),
    ) -> Result<(), traits::Error> {
        let mut sections: Vec<IndexedSection<'_>> = Vec::new();
        let mut code_section: Option<CodeSection<'_>> = None;
        let mut function_section: Option<FunctionSection<'_>> = None;
        let mut sizes: HashMap<usize, u32> = HashMap::new();

        // The function and code sections must be handled differently, so these
        // are not placed in the same `sections` array as the rest.
        let mut idx = 0;
        // @@@ revert to old style.
        loop {
            let start = self.current_position();
            let at_eof = self.offset == self.data.len();
            if at_eof {
                break;
            }
            let (section, bytes_consumed) =
                match self.parser.parse(&self.data[self.offset..], at_eof)? {
                    wasmparser::Chunk::NeedMoreData { .. } => {
                        panic!("@@@ nyi");
                    }
                    wasmparser::Chunk::Parsed { consumed, payload } => (payload, consumed),
                };
            self.offset += bytes_consumed;
            let size = self.current_position() - start; // @@@ refactor this nonsense
            let indexed_section = IndexedSection(idx, section); // @@@ bananas
            match indexed_section.1 {
                wasmparser::Payload::CodeSectionStart { range, .. } => {
                    code_section = Some(self.new_code_section(idx, start, range)?);
                }
                wasmparser::Payload::FunctionSection(reader) => {
                    function_section = Some(FunctionSection {
                        index: idx,
                        byte_size: reader.range().end - start,
                        reader: reader,
                    });
                }
                wasmparser::Payload::CodeSectionEntry { .. } => {
                    // Ignore.
                }
                _ => sections.push(indexed_section),
            };
            sizes.insert(idx, size as u32); // @@@ delete if unused.
            idx += 1; // @@@ wrong. we're counting any payload as a section.
        }

        // Before we actually parse any items prepare to parse a few sections
        // below, namely the code section. When parsing the code section we want
        // to try to assign human-readable names so we need the name section, if
        // present. Additionally we need to look at the number of imported
        // functions to handle the wasm function index space correctly.
        let names = parse_names_section(&sections)?;
        let imported_functions = count_imported_functions(&sections)?;

        // Next, we parse the function and code sections together, so that we
        // can collapse corresponding entries from the code and function
        // sections into a single representative IR item.
        match (function_section, code_section) {
            (Some(function_section), Some(code_section)) => {
                (function_section, code_section).parse_items(items, (imported_functions, &names))?
            }
            _ => Err(traits::Error::with_msg(
                "function or code section is missing",
            ))?,
        };

        for IndexedSection(idx, section) in sections.into_iter() {
            let start = items.size_added();
            let name = get_section_name(&section);
            match section {
                wasmparser::Payload::CustomSection {
                    name,
                    data,
                    data_offset,
                } => {
                    CustomSectionReader {
                        name,
                        data,
                        data_offset,
                    }
                    .parse_items(items, idx)?;
                }
                wasmparser::Payload::TypeSection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::ImportSection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::TableSection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::MemorySection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::GlobalSection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::ExportSection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::StartSection { func, range } => {
                    StartSection {
                        function_index: func,
                        data: &self.data[range.start..range.end],
                    }
                    .parse_items(items, idx)?;
                }
                wasmparser::Payload::ElementSection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::DataSection(mut reader) => {
                    reader.parse_items(items, idx)?;
                }
                wasmparser::Payload::DataCountSection { range, .. } => {
                    DataCountSection {
                        size: range.end - range.start,
                    }
                    .parse_items(items, idx)?;
                }
                wasmparser::Payload::CodeSectionStart { .. }
                | wasmparser::Payload::FunctionSection(_) => {
                    unreachable!("unexpected code or function section found");
                }

                _ => {} // @@@
            };
            let id = Id::section(idx);
            let added = items.size_added() - start;
            let size = sizes
                .get(&idx)
                .ok_or_else(|| traits::Error::with_msg("Could not find section size"))?;
            assert!(added <= *size);
            items.add_root(ir::Item::new(id, name, size - added, ir::Misc::new()));
        }

        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        _extra: (),
    ) -> Result<(), traits::Error> {
        let mut sections: Vec<IndexedSection<'_>> = Vec::new();
        let mut code_section: Option<CodeSection<'a>> = None;
        let mut function_section: Option<FunctionSection<'a>> = None;

        let mut idx = 0;
        while !self.eof() {
            let section = self.read()?;
            let start = self.current_position();
            match section {
                wasmparser::Payload::CodeSectionStart { range, .. } => {
                    code_section = Some(self.new_code_section(idx, start, range)?);
                }
                wasmparser::Payload::FunctionSection(reader) => {
                    function_section = Some(FunctionSection {
                        index: idx,
                        byte_size: reader.range().end - start,
                        reader: reader,
                    });
                }
                _ => sections.push(IndexedSection(idx, section)),
            };
            idx += 1;
        }

        // Like above we do some preprocessing here before actually drawing all
        // the edges below. Here we primarily want to learn some properties of
        // the wasm module, such as what `Id` is mapped to all index spaces in
        // the wasm module. To handle that we build up all this data in
        // `SectionIndices` here as we parse all the various sections.
        let mut indices = SectionIndices::default();
        for IndexedSection(idx, section) in sections.iter() {
            match section {
                wasmparser::Payload::TypeSection(_reader) => {
                    indices.type_ = Some(*idx);
                }
                wasmparser::Payload::ImportSection(reader) => {
                    for (i, import) in reader.clone().into_iter().enumerate() {
                        let id = Id::entry(*idx, i);
                        match import?.ty {
                            wasmparser::ImportSectionEntryType::Function(_) => {
                                indices.functions.push(id);
                            }
                            wasmparser::ImportSectionEntryType::Table(_) => {
                                indices.tables.push(id);
                            }
                            wasmparser::ImportSectionEntryType::Memory(_) => {
                                indices.memories.push(id);
                            }
                            wasmparser::ImportSectionEntryType::Global(_) => {
                                indices.globals.push(id);
                            }
                            wasmparser::ImportSectionEntryType::Event(_) => {} // @@@
                            wasmparser::ImportSectionEntryType::Module(_) => {} // @@@
                            wasmparser::ImportSectionEntryType::Instance(_) => {} // @@@
                        }
                    }
                }
                wasmparser::Payload::GlobalSection(reader) => {
                    for i in 0..reader.get_count() {
                        let id = Id::entry(*idx, i as usize);
                        indices.globals.push(id);
                    }
                }
                wasmparser::Payload::MemorySection(reader) => {
                    for i in 0..reader.get_count() {
                        let id = Id::entry(*idx, i as usize);
                        indices.memories.push(id);
                    }
                }
                wasmparser::Payload::TableSection(reader) => {
                    for i in 0..reader.get_count() {
                        let id = Id::entry(*idx, i as usize);
                        indices.tables.push(id);
                    }
                }
                wasmparser::Payload::CodeSectionStart { .. } => {
                    Err(traits::Error::with_msg("unexpected code section"))?
                }
                wasmparser::Payload::FunctionSection(_reader) => {
                    Err(traits::Error::with_msg("unexpected function section"))?
                }
                _ => {}
            }
        }
        if let (Some(function_section), Some(code_section)) =
            (function_section.as_ref(), code_section.as_ref())
        {
            indices.code = Some(code_section.index);
            for i in 0..function_section.reader.get_count() {
                let id = Id::entry(code_section.index, i as usize);
                indices.functions.push(id);
            }
        }

        match (function_section, code_section) {
            (Some(function_section), Some(code_section)) => {
                (function_section, code_section).parse_edges(items, &indices)?
            }
            _ => panic!("function or code section is missing"),
        };
        for IndexedSection(idx, section) in sections.into_iter() {
            match section {
                wasmparser::Payload::CustomSection {
                    name,
                    data,
                    data_offset,
                } => {
                    CustomSectionReader {
                        name,
                        data,
                        data_offset,
                    }
                    .parse_edges(items, ())?;
                }
                wasmparser::Payload::TypeSection(mut reader) => {
                    reader.parse_edges(items, ())?;
                }
                wasmparser::Payload::ImportSection(mut reader) => {
                    reader.parse_edges(items, ())?;
                }
                wasmparser::Payload::TableSection(mut reader) => {
                    reader.parse_edges(items, ())?;
                }
                wasmparser::Payload::MemorySection(mut reader) => {
                    reader.parse_edges(items, ())?;
                }
                wasmparser::Payload::GlobalSection(mut reader) => {
                    reader.parse_edges(items, ())?;
                }
                wasmparser::Payload::ExportSection(mut reader) => {
                    reader.parse_edges(items, (&indices, idx))?;
                }
                wasmparser::Payload::StartSection { func, range } => {
                    StartSection {
                        function_index: func,
                        data: &self.data[range.start..range.end],
                    }
                    .parse_edges(items, (&indices, idx))?;
                }
                wasmparser::Payload::ElementSection(mut reader) => {
                    reader.parse_edges(items, (&indices, idx))?;
                }
                wasmparser::Payload::DataSection(mut reader) => {
                    reader.parse_edges(items, ())?;
                }
                wasmparser::Payload::DataCountSection { range, .. } => {
                    DataCountSection {
                        size: range.end - range.start,
                    }
                    .parse_edges(items, ())?;
                }
                wasmparser::Payload::CodeSectionStart { .. }
                | wasmparser::Payload::FunctionSection { .. } => {
                    unreachable!("unexpected code or function section found");
                }

                _ => {} // @@@
            }
        }

        Ok(())
    }
}

fn get_code_section_name() -> String {
    "code section headers".to_string()
}

fn get_section_name(section: &wasmparser::Payload<'_>) -> String {
    match section {
        wasmparser::Payload::CustomSection { name, .. } => {
            format!("custom section '{}' headers", name)
        }
        wasmparser::Payload::TypeSection(_) => "type section headers".to_string(),
        wasmparser::Payload::ImportSection(_) => "import section headers".to_string(),
        wasmparser::Payload::FunctionSection(_) => "function section headers".to_string(),
        wasmparser::Payload::TableSection(_) => "table section headers".to_string(),
        wasmparser::Payload::MemorySection(_) => "memory section headers".to_string(),
        wasmparser::Payload::GlobalSection(_) => "global section headers".to_string(),
        wasmparser::Payload::ExportSection(_) => "export section headers".to_string(),
        wasmparser::Payload::StartSection { .. } => "start section headers".to_string(),
        wasmparser::Payload::ElementSection(_) => "element section headers".to_string(),
        wasmparser::Payload::CodeSectionStart { .. } => get_code_section_name(),
        wasmparser::Payload::DataSection(_) => "data section headers".to_string(),
        wasmparser::Payload::DataCountSection { .. } => "data count section headers".to_string(),
        wasmparser::Payload::Version { .. } => "wasm magic bytes".to_string(),

        wasmparser::Payload::CodeSectionEntry { .. } => {
            panic!("unexpected CodeSectionEntry");
        }

        _ => format!("@@@ {:?}", section), // @@@
    }
}

fn parse_names_section<'a>(
    indexed_sections: &[IndexedSection<'a>],
) -> Result<HashMap<usize, &'a str>, traits::Error> {
    let mut names = HashMap::new();
    for IndexedSection(_, section) in indexed_sections.iter() {
        if let wasmparser::Payload::CustomSection {
            name: "name",
            data,
            data_offset,
        } = section
        {
            for subsection in wasmparser::NameSectionReader::new(data, *data_offset)? {
                let f = match subsection? {
                    wasmparser::Name::Function(f) => f,
                    _ => continue,
                };
                let mut map = f.get_map()?;
                for _ in 0..map.get_count() {
                    let naming = map.read()?;
                    names.insert(naming.index as usize, naming.name);
                }
            }
        }
    }
    Ok(names)
}

fn count_imported_functions<'a>(
    indexed_sections: &[IndexedSection<'a>],
) -> Result<usize, traits::Error> {
    let mut imported_functions = 0;
    for IndexedSection(_, section) in indexed_sections.iter() {
        if let wasmparser::Payload::ImportSection(reader) = section {
            for import in reader.clone() {
                if let wasmparser::ImportSectionEntryType::Function(_) = import?.ty {
                    imported_functions += 1;
                }
            }
        }
    }
    Ok(imported_functions)
}

impl<'a> Parse<'a> for (FunctionSection<'a>, CodeSection<'a>) {
    type ItemsExtra = (usize, &'a HashMap<usize, &'a str>);

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (imported_functions, names): Self::ItemsExtra,
    ) -> Result<(), traits::Error> {
        let (func_section, code_section) = self;

        let func_section_index = func_section.index;
        let func_items: Vec<ir::Item> = iterate_with_size(&mut func_section.reader)
            .enumerate()
            .map(|(i, func)| {
                let (_func, size) = func?;
                let id = Id::entry(func_section_index, i);
                let name = format!("func[{}]", i);
                let item = ir::Item::new(id, name, size, ir::Misc::new());
                Ok(item)
            })
            .collect::<Result<_, traits::Error>>()?;

        let code_section_index = code_section.index;
        let code_items: Vec<ir::Item> = iterate_with_size(&mut code_section.reader)
            .zip(func_items.into_iter())
            .enumerate()
            .map(|(i, (body, func))| {
                let (_body, size) = body?;
                let id = Id::entry(code_section_index, i);
                let name = names
                    .get(&(i + imported_functions))
                    .map_or_else(|| format!("code[{}]", i), |name| name.to_string());
                let code = ir::Code::new(&name);
                let item = ir::Item::new(id, name, size + func.size(), code);
                Ok(item)
            })
            .collect::<Result<_, traits::Error>>()?;

        let start = items.size_added();
        let name = get_code_section_name();
        for item in code_items.into_iter() {
            items.add_item(item);
        }
        let id = Id::section(code_section.index);
        let added = items.size_added() - start;
        let code_section_size = code_section.byte_size as u32;
        let func_section_size = func_section.byte_size as u32;
        // @@@ bye
        //let code_section_size = sizes
        //    .get(&code_section.index)
        //    .ok_or_else(|| traits::Error::with_msg("Could not find section size"))?;
        //let func_section_size = sizes
        //        .get(&func_section.index)
        //        .ok_or_else(|| traits::Error::with_msg("Could not find section size"))?;
        let size = code_section_size + func_section_size;
        //println!("size={} added={} code_section_size={} func_section_size={}", size, added, code_section_size, func_section_size);
        //println!("sizes={:#?}", sizes);
        assert!(added <= size);
        items.add_root(ir::Item::new(id, name, size - added, ir::Misc::new()));

        Ok(())
    }

    type EdgesExtra = &'a SectionIndices;

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        indices: Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        let (function_section, code_section) = self;

        type Edge = (ir::Id, ir::Id);

        let mut edges: Vec<Edge> = Vec::new();

        // Function section reader parsing.
        for (func_i, type_ref) in iterate_with_size(&mut function_section.reader).enumerate() {
            let (type_ref, _) = type_ref?;
            if let Some(type_idx) = indices.type_ {
                let type_id = Id::entry(type_idx, type_ref as usize);
                if let Some(code_idx) = indices.code {
                    let body_id = Id::entry(code_idx, func_i);
                    edges.push((body_id, type_id));
                }
            }
        }

        // Code section reader parsing.
        for (b_i, body) in iterate_with_size(&mut code_section.reader).enumerate() {
            let (body, _size) = body?;
            let body_id = Id::entry(code_section.index, b_i);

            let mut cache = None;
            for op in body.get_operators_reader()? {
                let prev = cache.take();
                match op? {
                    Operator::Call { function_index } => {
                        let f_id = indices.functions[function_index as usize];
                        edges.push((body_id, f_id));
                    }

                    // TODO: Rather than looking at indirect calls, need to look
                    // at where the vtables get initialized and/or vtable
                    // indices get pushed onto the stack.
                    Operator::CallIndirect { .. } => continue,

                    Operator::GlobalGet { global_index } | Operator::GlobalSet { global_index } => {
                        let g_id = indices.globals[global_index as usize];
                        edges.push((body_id, g_id));
                    }

                    Operator::I32Load { memarg }
                    | Operator::I32Load8S { memarg }
                    | Operator::I32Load8U { memarg }
                    | Operator::I32Load16S { memarg }
                    | Operator::I32Load16U { memarg }
                    | Operator::I64Load { memarg }
                    | Operator::I64Load8S { memarg }
                    | Operator::I64Load8U { memarg }
                    | Operator::I64Load16S { memarg }
                    | Operator::I64Load16U { memarg }
                    | Operator::I64Load32S { memarg }
                    | Operator::I64Load32U { memarg }
                    | Operator::F32Load { memarg }
                    | Operator::F64Load { memarg } => {
                        if let Some(Operator::I32Const { value }) = prev {
                            if let Some(data_id) = items.get_data(value as u32 + memarg.offset) {
                                edges.push((body_id, data_id));
                            }
                        }
                    }
                    other => cache = Some(other),
                }
            }
        }

        edges
            .into_iter()
            .for_each(|(from, to)| items.add_edge(from, to));

        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::NameSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        let mut i = 0;
        while !self.eof() {
            let start = self.original_position();
            let subsection = self.read()?;
            let size = (self.original_position() - start) as u32;
            let name = match subsection {
                wasmparser::Name::Module(_) => "\"module name\" subsection",
                wasmparser::Name::Function(_) => "\"function names\" subsection",
                wasmparser::Name::Local(_) => "\"local names\" subsection",
            };
            let id = Id::entry(idx, i);
            items.add_root(ir::Item::new(id, name, size, ir::DebugInfo::new()));
            i += 1;
        }

        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

struct CustomSectionReader<'a> {
    name: &'a str,
    data: &'a [u8],
    data_offset: usize,
}

impl<'a> Parse<'a> for CustomSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        if self.name == "name" {
            wasmparser::NameSectionReader::new(self.data, self.data_offset)?
                .parse_items(items, idx)?;
        } else {
            let size = self.data.len() as u32;
            let id = Id::entry(idx, 0);
            let name = format!("custom section '{}'", self.name);
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::TypeSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, ty) in iterate_with_size(self).enumerate() {
            let (ty, size) = ty?;
            let id = Id::entry(idx, i);

            match ty {
                wasmparser::TypeDef::Func(func) => {
                    let mut name = format!("type[{}]: (", i);
                    for (i, param) in func.params.iter().enumerate() {
                        if i != 0 {
                            name.push_str(", ");
                        }
                        name.push_str(ty2str(*param));
                    }
                    name.push_str(") -> ");

                    match func.returns.len() {
                        0 => name.push_str("nil"),
                        1 => name.push_str(ty2str(func.returns[0])),
                        _ => {
                            name.push_str("(");
                            for (i, result) in func.returns.iter().enumerate() {
                                if i != 0 {
                                    name.push_str(", ");
                                }
                                name.push_str(ty2str(*result));
                            }
                            name.push_str(")");
                        }
                    }

                    items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
                }
                // @@@ Is this correct?
                wasmparser::TypeDef::Module(_module) => {}
                wasmparser::TypeDef::Instance(_instance) => {}
            }
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::ImportSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, imp) in iterate_with_size(self).enumerate() {
            let (imp, size) = imp?;
            let id = Id::entry(idx, i);
            let name = format!("import {}::{}", imp.module, imp.field.unwrap_or("@@@"));
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, (): ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::TableSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, entry) in iterate_with_size(self).enumerate() {
            let (_entry, size) = entry?;
            let id = Id::entry(idx, i);
            let name = format!("table[{}]", i);
            items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::MemorySectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, mem) in iterate_with_size(self).enumerate() {
            let (_mem, size) = mem?;
            let id = Id::entry(idx, i);
            let name = format!("memory[{}]", i);
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::GlobalSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, g) in iterate_with_size(self).enumerate() {
            let (g, size) = g?;
            let id = Id::entry(idx, i);
            let name = format!("global[{}]", i);
            let ty = ty2str(g.ty.content_type).to_string();
            items.add_item(ir::Item::new(id, name, size, ir::Data::new(Some(ty))));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::ExportSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, exp) in iterate_with_size(self).enumerate() {
            let (exp, size) = exp?;
            let id = Id::entry(idx, i);
            let name = format!("export \"{}\"", exp.field);
            items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = (&'a SectionIndices, usize);

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (indices, idx): Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        for (i, exp) in iterate_with_size(self).enumerate() {
            let (exp, _) = exp?;
            let exp_id = Id::entry(idx, i);
            match exp.kind {
                wasmparser::ExternalKind::Function => {
                    items.add_edge(exp_id, indices.functions[exp.index as usize]);
                }
                wasmparser::ExternalKind::Table => {
                    items.add_edge(exp_id, indices.tables[exp.index as usize]);
                }
                wasmparser::ExternalKind::Memory => {
                    items.add_edge(exp_id, indices.memories[exp.index as usize]);
                }
                wasmparser::ExternalKind::Global => {
                    items.add_edge(exp_id, indices.globals[exp.index as usize]);
                }
                wasmparser::ExternalKind::Event
                | wasmparser::ExternalKind::Type
                | wasmparser::ExternalKind::Module
                | wasmparser::ExternalKind::Instance => {} // @@@
            }
        }

        Ok(())
    }
}

struct StartSection<'a> {
    function_index: u32,
    data: &'a [u8], // @@@ we only need the size.
}

impl<'a> Parse<'a> for StartSection<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        let size = self.data.len() as u32;
        let id = Id::section(idx);
        let name = "\"start\" section";
        items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        Ok(())
    }

    type EdgesExtra = (&'a SectionIndices, usize);

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (indices, idx): Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        items.add_edge(
            Id::section(idx),
            indices.functions[self.function_index as usize],
        );
        Ok(())
    }
}

struct DataCountSection {
    size: usize,
}

impl<'a> Parse<'a> for DataCountSection {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        let size = self.size as u32;
        let id = Id::section(idx);
        let name = "\"data count\" section";
        items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _items: &mut ir::ItemsBuilder, (): ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::ElementSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, elem) in iterate_with_size(self).enumerate() {
            let (_elem, size) = elem?;
            let id = Id::entry(idx, i);
            let name = format!("elem[{}]", i);
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = (&'a SectionIndices, usize);

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (indices, idx): Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        for (i, elem) in iterate_with_size(self).enumerate() {
            let (elem, _size) = elem?;
            let elem_id = Id::entry(idx, i);

            match elem.kind {
                wasmparser::ElementKind::Active { table_index, .. } => {
                    items.add_edge(indices.tables[table_index as usize], elem_id);
                }
                wasmparser::ElementKind::Declared => {}
                wasmparser::ElementKind::Passive => {}
            }
            for element_item in elem.items.get_items_reader()? {
                let element_item = element_item?;
                match element_item {
                    wasmparser::ElementItem::Func(func_idx) => {
                        items.add_edge(elem_id, indices.functions[func_idx as usize]);
                    }
                    wasmparser::ElementItem::Null(_ty) => {}
                }
            }
        }

        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::DataSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, d) in iterate_with_size(self).enumerate() {
            let (d, size) = d?;
            let id = Id::entry(idx, i);
            let name = format!("data[{}]", i);
            items.add_item(ir::Item::new(id, name, size, ir::Data::new(None)));

            // Get the constant address (if any) from the initialization
            // expression.
            if let wasmparser::DataKind::Active { init_expr, .. } = d.kind {
                let mut iter = init_expr.get_operators_reader();
                let offset = match iter.read()? {
                    Operator::I32Const { value } => Some(i64::from(value)),
                    Operator::I64Const { value } => Some(value),
                    _ => None,
                };

                if let Some(off) = offset {
                    let length = d.data.len(); // size of data
                    items.link_data(off, length, id);
                }
            }
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

fn iterate_with_size<'a, S: SectionWithLimitedItems + SectionReader>(
    s: &'a mut S,
) -> impl Iterator<Item = Result<(S::Item, u32), traits::Error>> + 'a {
    let count = s.get_count();
    (0..count).map(move |i| {
        let start = s.original_position();
        let item = s.read()?;
        let size = (s.original_position() - start) as u32;
        if i == count - 1 {
            s.ensure_end()?;
        }
        Ok((item, size))
    })
}

fn ty2str(t: Type) -> &'static str {
    match t {
        Type::I32 => "i32",
        Type::I64 => "i64",
        Type::F32 => "f32",
        Type::F64 => "f64",
        Type::V128 => "v128",
        Type::ExnRef => "exnref",    // @@@
        Type::FuncRef => "anyfunc",  // @@@ rename?
        Type::ExternRef => "anyref", // @@@ rename?
        Type::Func | Type::EmptyBlockType => "?",
    }
}
