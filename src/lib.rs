pub mod bibmechanics;
pub mod raw;
pub mod types;

mod resolve;

use std::collections::HashMap;

use anyhow::anyhow;
use paste::paste;

use crate::bibmechanics::{AuthorMode, EntryType, PagesChapterMode};
use crate::raw::RawBibliography;
use crate::resolve::resolve;
use crate::types::{
    chunks_to_string, Date, EditorType, Gender, IntOrChunks, Pagination, Person, Type,
};

/// A fully parsed bibliography.
#[derive(Clone, Debug)]
pub struct Bibliography {
    items: Vec<Entry>,
    dict: HashMap<String, usize>,
}

/// A bibliography entry that is parsed into chunks, which can be
/// parsed into more specific types on demand on field accesses.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Entry {
    /// The citation key.
    pub cite_key: String,
    /// Denotes the type of bibliography item (e.g. `article`).
    pub entry_type: EntryType,
    /// Maps from field names to their associated chunk vectors.
    pub fields: HashMap<String, Vec<Chunk>>,
}

/// A chunk represents one part of a field value.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Chunk {
    /// Normal values within quotes or single braces subject to
    /// capitalization formatting.
    Normal(String),
    /// Values nested in braces that are to be printed like specified
    /// in the file. Escapes keywords.
    ///
    /// Example: `"Inside {NASA}"` or `{Memes are {gReAT}}`.
    Verbatim(String),
}

impl Bibliography {
    /// Get a new, empty Bibliography
    pub fn new() -> Self {
        Self { items: Vec::new(), dict: HashMap::new() }
    }

    /// Parse a bibliography from a source string.
    pub fn from_str(src: &str, allow_bibtex: bool) -> Self {
        Self::from_raw(RawBibliography::from_str(src, allow_bibtex))
    }

    /// Parse a bibliography from a raw bibliography.
    pub fn from_raw(raw: RawBibliography) -> Self {
        let mut res = Self::new();
        let abbreviations = &raw.abbreviations;

        for entry in raw.entries {
            res.add(Entry {
                cite_key: entry.cite_key.to_string(),
                entry_type: EntryType::robust_from_str(entry.entry_type),
                fields: entry
                    .fields
                    .into_iter()
                    .map(|(key, value)| (key.to_string(), resolve(value, abbreviations)))
                    .collect(),
            })
            .ok();
        }

        res
    }

    /// Try to find an entry with the given cite key.
    pub fn get(&self, cite_key: &str) -> Option<&Entry> {
        let index = self.dict.get(cite_key);
        if let Some(&index) = index {
            self.items.get(index)
        } else {
            None
        }
    }

    /// Try to find an entry with the given cite key and return a mutable reference.
    pub fn get_mut(&mut self, cite_key: &str) -> Option<&mut Entry> {
        let index = self.dict.get(cite_key);
        if let Some(&index) = index {
            self.items.get_mut(index)
        } else {
            None
        }
    }

    /// Try to find an entry with the given cite key and return an Entry
    /// with all `crossref` and `xdata` dependencies resolved.
    pub fn get_resolved(&mut self, cite_key: &str) -> Option<Entry> {
        self.get_mut(cite_key)
            .cloned()
            .and_then(|mut e| e.resolve_crossrefs(self).map(|_| e).ok())
    }

    /// Add a new entry of that type to the bibliography if the cite key
    /// is not already in use.
    pub fn add(&mut self, entry: Entry) -> anyhow::Result<()> {
        if self.get(&entry.cite_key).is_some() {
            Err(anyhow!("key already present"))
        } else {
            self.dict.insert(entry.cite_key.clone(), self.items.len());
            self.items.push(entry);
            let ids = self.items.last().unwrap().get_as::<Vec<String>>("ids");
            let key = self.items.last().unwrap().cite_key.clone();
            if let Ok(ids) = ids {
                for i in ids {
                    self.add_alias(&key, &i)?;
                }
            }
            Ok(())
        }
    }

    /// Add a new entry of that type to the bibliography if the cite key
    /// is not already in use.
    pub fn add_empty(
        &mut self,
        cite_key: &str,
        entry_type: EntryType,
    ) -> anyhow::Result<&mut Entry> {
        if self.get(cite_key).is_some() {
            Err(anyhow!("key already present"))
        } else {
            self.dict.insert(cite_key.to_string(), self.items.len());
            self.items.push(Entry::new(cite_key, entry_type));
            self.get_mut(cite_key)
                .ok_or_else(|| anyhow!("could not fetch inserted entry"))
        }
    }

    /// Will add an alias for a cite key if that alias is not yet in use
    pub fn add_alias(&mut self, cite_key: &str, alias: &str) -> anyhow::Result<()> {
        let &index = self
            .dict
            .get(cite_key)
            .ok_or_else(|| anyhow!("item for alias not found"))?;
        if self.dict.contains_key(alias) {
            Err(anyhow!("alias name already in use"))
        } else {
            self.dict.insert(alias.to_string(), index);
            Ok(())
        }
    }

    pub fn remove_item(&mut self, cite_key: &str) -> anyhow::Result<Entry> {
        let &index = self
            .dict
            .get(cite_key)
            .ok_or_else(|| anyhow!("item for alias not found"))?;
        let entry = self.items.remove(index);

        self.dict.retain(|_, &mut v| v != index);
        self.dict = self
            .dict
            .clone()
            .into_iter()
            .map(|(k, mut v)| {
                if v > index {
                    v -= 1;
                }

                (k, v)
            })
            .collect();

        Ok(entry)
    }

    /// Get the amount of bibliography items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// An iterator over the bibliography's entries.
    pub fn iter<'a>(&'a self) -> std::slice::Iter<'a, Entry> {
        self.items.iter()
    }

    /// Will output the entry as a BibLaTeX string
    pub fn as_biblatex_string(&mut self) -> String {
        let mut res = String::new();
        for e in self.items.iter_mut() {
            res += &e.as_biblatex_string();
            res.push('\n')
        }

        res
    }
}

impl IntoIterator for Bibliography {
    type Item = Entry;
    type IntoIter = std::vec::IntoIter<Entry>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl Entry {
    /// Construct new, empty entry.
    pub fn new(cite_key: &str, entry_type: EntryType) -> Self {
        Self {
            cite_key: cite_key.to_string(),
            entry_type,
            fields: HashMap::new(),
        }
    }

    /// Get the chunk slice for a field.
    pub fn get(&self, key: &str) -> Option<&[Chunk]> {
        self.fields.get(&key.to_lowercase()).map(AsRef::as_ref)
    }

    /// Get the chunk slice for a field and parse it as a specific type.
    pub fn get_as<T: Type>(&self, key: &str) -> anyhow::Result<T> {
        self.get(key)
            .ok_or_else(|| anyhow!("The {} field is not present", key))
            .and_then(|chunks| chunks.parse::<T>())
    }

    /// Get an entry but return None for empty chunk slices.
    fn get_non_empty(&self, key: &str) -> Option<&[Chunk]> {
        self.get(key).and_then(|f| if f.is_empty() { None } else { Some(f) })
    }

    /// Sets a field value as a chunk vector.
    pub fn set(&mut self, key: &str, chunks: Vec<Chunk>) {
        self.fields.insert(key.to_lowercase(), chunks);
    }

    /// Sets a field value as a chunk vector as a specific type.
    pub fn set_as<T: Type>(&mut self, key: &str, value: &T) -> anyhow::Result<()> {
        let chunks = value.to_chunks()?;
        self.fields.insert(key.to_lowercase(), chunks);
        Ok(())
    }

    /// Deletes a field from the entry.
    pub fn delete(&mut self, key: &str) -> Option<Vec<Chunk>> {
        self.fields.remove(key)
    }

    /// Renames a field if present.
    fn rename(&mut self, old_key: &str, new_key: &str) {
        if let Some(chunks) = self.delete(old_key) {
            self.set(new_key, chunks);
        }
    }

    /// Will output the entry as a BibLaTeX string.
    pub fn as_biblatex_string(&mut self) -> String {
        self.rename("journal", "journaltitle");
        self.rename("address", "location");
        self.rename("school", "institution");
        let mut res = format!("@{}{{{},\n", self.entry_type.to_biblatex(), self.cite_key);
        for (key, value) in self.fields.iter() {
            res.push_str(&format!("{} = {},\n", key, chunks_to_string(value)))
        }

        res.push('}');

        res
    }

    /// Will output the entry as a BibTeX string.
    pub fn as_bibtex_string(&mut self) -> String {
        let bibtex_type = self.entry_type.to_bibtex();
        self.rename("journaltitle", "journal");
        self.rename("location", "address");
        if bibtex_type == EntryType::PhdThesis || bibtex_type == EntryType::MastersThesis
        {
            self.rename("institution", "school");
        }
        if let Ok(date) = self.get_date() {
            for (key, value) in date.to_fieldset() {
                self.set(&key, vec![Chunk::Normal(value)]);
            }
        }

        let mut res = format!("@{}{{{},\n", bibtex_type, self.cite_key);
        for (key, value) in self.fields.iter() {
            if key == "date" {
                continue;
            }

            res.push_str(&format!("{} = {},\n", key, chunks_to_string(value)))
        }

        res.push('}');

        res
    }

    /// Gets the parents of an entry in a semantic sense (`crossref` and `xref`).
    pub fn get_parents(&self) -> Vec<String> {
        let mut res = vec![];

        if let Ok(crossref) = self.get_as::<String>("crossref") {
            res.push(crossref);
        }

        if let Ok(mut xrefs) = self.get_as::<Vec<String>>("xref") {
            res.append(&mut xrefs);
        }

        res
    }

    /// Resolve data dependencies using another entry.
    fn resolve_single_crossref(&mut self, crossref: Entry) {
        let typ = self.entry_type.clone();
        let mut requirements = typ.get_requirements();
        let mut active_fields = requirements.required.clone();
        active_fields.append(&mut requirements.optional);
        active_fields.append(&mut requirements.page_chapter_field.get_all_possible());
        active_fields.append(&mut requirements.author_eds_field.get_all_possible());

        if self.entry_type == EntryType::XData {
            for f in crossref.fields.keys() {
                active_fields.push(f);
            }
        }

        for f in active_fields {
            if self.get(f).is_some() {
                continue;
            }

            match f {
                "journaltitle" | "journalsubtitle"
                    if crossref.entry_type == EntryType::Periodical =>
                {
                    let key = if f.contains('s') { "subtitle" } else { "title" };

                    if let Some(item) = crossref.get(key) {
                        self.set(f, item.to_vec())
                    }
                }
                "booktitle" | "booksubtitle" | "booktitleaddon"
                    if crossref.entry_type.is_collection() =>
                {
                    let key = if f.contains('s') {
                        "subtitle"
                    } else if f.contains('a') {
                        "titleaddon"
                    } else {
                        "title"
                    };

                    if let Some(item) = crossref.get(key) {
                        self.set(f, item.to_vec())
                    }
                }
                "maintitle" | "mainsubtitle" | "maintitleaddon"
                    if crossref.entry_type.is_multi_volume() =>
                {
                    let key = if f.contains('s') {
                        "subtitle"
                    } else if f.contains('a') {
                        "titleaddon"
                    } else {
                        "title"
                    };

                    if let Some(item) = crossref.get(key) {
                        self.set(f, item.to_vec())
                    }
                }
                "address" => {
                    if let Some(item) =
                        crossref.get(f).or_else(|| crossref.get("location"))
                    {
                        self.set(f, item.to_vec())
                    }
                }
                "institution" => {
                    if let Some(item) = crossref.get(f).or_else(|| crossref.get("school"))
                    {
                        self.set(f, item.to_vec())
                    }
                }
                "school" => {
                    if let Some(item) =
                        crossref.get(f).or_else(|| crossref.get("institution"))
                    {
                        self.set(f, item.to_vec())
                    }
                }
                "journaltitle" => {
                    if let Some(item) =
                        crossref.get(f).or_else(|| crossref.get("journal"))
                    {
                        self.set(f, item.to_vec())
                    }
                }
                "title" | "addendum" | "note" => {}
                _ => {
                    if let Some(item) = crossref.get(f) {
                        self.set(f, item.to_vec())
                    }
                }
            }
        }

        if self.entry_type == EntryType::XData {
            return;
        }

        if requirements.needs_date {
            if let Ok(date) = crossref.get_date() {
                self.set_date(date).expect("date set failure");
            }
        }
    }

    /// Resolves all data dependancies defined by `crossref` and `xdata` fields.
    fn resolve_crossrefs(&mut self, bib: &mut Bibliography) -> anyhow::Result<()> {
        let crossref = self.get_as::<String>("crossref").map(|s| {
            bib.get_mut(&s)
                .map(|e| e.clone())
                .ok_or_else(|| anyhow!("crossref'd item not found"))
        });
        let references = self.get_as::<Vec<String>>("xdata").map(|keys| {
            keys.iter()
                .map(|s| {
                    bib.get_mut(&s)
                        .map(|e| e.clone())
                        .ok_or_else(|| anyhow!("crossref'd item not found"))
                })
                .collect::<Vec<anyhow::Result<Entry>>>()
        });

        let mut refs = vec![];

        if let Ok(crossref) = crossref {
            refs.push(crossref?);
        }

        if let Ok(references) = references {
            for r in references {
                refs.push(r?);
            }
        }

        for mut crossref in refs {
            crossref.resolve_crossrefs(bib)?;
            self.resolve_single_crossref(crossref);
        }

        self.delete("crossref");
        self.delete("xdata");

        Ok(())
    }

    /// Verify if the entry has all fields required for its `EntryType`.
    ///
    /// This function will return `Ok(())` upon successful validation.
    /// The Err variant of the Result will contain a pair carrying two vectors
    /// of keys: The first indicating fields that should have been present
    /// but were not, the second indicating fields that were set but are forbidden
    /// by the `EntryType`.
    ///
    /// **NOTE:** This function will not resolve the Entry.
    /// If you want to support / expect files using `crossref` and `xdata`,
    /// you will want to call this method only on entries obtained through the
    /// `Bibliography::get_resolved` call.
    pub fn verify(&self) -> Result<(), (Vec<&str>, Vec<&str>)> {
        let reqs = self.entry_type.get_requirements();
        let mut expected = vec![];
        let mut outlawed = vec![];

        for field in reqs.required {
            match field {
                "journaltitle" => {
                    if self
                        .get_non_empty(field)
                        .or_else(|| self.get_non_empty("journal"))
                        .is_none()
                    {
                        expected.push(field);
                    }
                }
                "location" => {
                    if self
                        .get_non_empty(field)
                        .or_else(|| self.get_non_empty("address"))
                        .is_none()
                    {
                        expected.push(field);
                    }
                }
                "school"
                    if self.entry_type == EntryType::Thesis
                        || self.entry_type == EntryType::MastersThesis
                        || self.entry_type == EntryType::PhdThesis =>
                {
                    if self
                        .get_non_empty(field)
                        .or_else(|| self.get_non_empty("institution"))
                        .is_none()
                    {
                        expected.push(field);
                    }
                }
                _ => {
                    if self.get_non_empty(field).is_none() {
                        expected.push(field);
                    }
                }
            }
        }

        for field in reqs.forbidden {
            if self.get_non_empty(field).is_some() {
                outlawed.push(field);
            }
        }

        if reqs.needs_date {
            if self.get_date().is_err() {
                expected.push("year");
            }
        }

        match reqs.page_chapter_field {
            PagesChapterMode::PagesRequired => {
                if self.get_pages().is_err() {
                    expected.push("pages");
                }
            }
            PagesChapterMode::PagesChapterRequired => {
                if self
                    .get_pages()
                    .map(|_| true)
                    .or_else(|_| self.get_chapter().map(|_| true))
                    .is_err()
                {
                    expected.push("pages");
                }
            }
            PagesChapterMode::BothForbidden => {
                if self.get_pages().is_ok() {
                    outlawed.push("pages");
                }
                if self.get_chapter().is_ok() {
                    outlawed.push("chapter");
                }
            }
            _ => {}
        }

        match reqs.author_eds_field {
            AuthorMode::AuthorRequired | AuthorMode::AuthorRequiredEditorOptional => {
                if self.get_author().is_err() {
                    expected.push("author");
                }
            }
            AuthorMode::AuthorEditorRequired => {
                if self
                    .get_author()
                    .map(|_| true)
                    .or_else(|_| self.get_editors().map(|_| true))
                    .is_err()
                {
                    expected.push("author");
                }
            }
            AuthorMode::EditorRequired => {
                if self.get_editors().is_err() {
                    expected.push("editor");
                }
            }
            AuthorMode::EditorRequiredAuthorForbidden => {
                if self.get_editors().is_err() {
                    expected.push("editor");
                }
                if self.get_author().is_ok() {
                    outlawed.push("author");
                }
            }
            AuthorMode::BothRequired => {
                if self.get_editors().is_err() {
                    expected.push("editor");
                }
                if self.get_author().is_err() {
                    expected.push("author");
                }
            }
            _ => {}
        }

        if expected.is_empty() && outlawed.is_empty() {
            Ok(())
        } else {
            Err((expected, outlawed))
        }
    }
}

macro_rules! fields {
    ($($name:ident: $field_name:expr $(=> $res:ty)?),* $(,)*) => {
        $(
            paste! {
                #[doc = "Get and parse the `" $field_name "` field."]
                pub fn [<get_ $name>](&self) -> anyhow::Result<fields!(@type $($res)?)> {
                    self.get($field_name)
                        .ok_or_else(|| anyhow!("The {} field is not present", $field_name))
                        $(.and_then(|chunks| chunks.parse::<$res>()))?
                }

                fields!(single_set $name => $field_name, $($res)?);
            }
        )*
    };

    (single_set $name:ident => $field_name:expr, ) => {
        paste! {
            #[doc = "Set a value in the `" $field_name "` field."]
            pub fn [<set_ $name>](&mut self, item: Vec<Chunk>) -> anyhow::Result<()> {
                self.set($field_name, item);
                Ok(())
            }
        }
    };
    (single_set $name:ident => $field_name:expr, $other_type:ty) => {
        paste! {
            #[doc = "Set a value in the `" $field_name "` field."]
            pub fn [<set_ $name>](&mut self, item: $other_type) -> anyhow::Result<()> {
                let chunks = item.to_chunks()?;
                self.set($field_name, chunks);
                Ok(())
            }
        }
    };

    (@type) => {&[Chunk]};
    (@type $res:ty) => {$res};
}

macro_rules! alias_fields {
    ($($name:ident: $field_name:expr, $field_alias:expr $(=> $res:ty)?),* $(,)*) => {
        $(
            paste! {
                #[doc = "Get and parse the `" $field_name "` field, falling back on `" $field_alias "` if `" $field_name "` is empty."]
                pub fn [<get_ $name>](&self) -> anyhow::Result<fields!(@type $($res)?)> {
                    self.get($field_name)
                        .or_else(|| self.get($field_alias))
                        .ok_or_else(|| anyhow!("The {} field is not present", $field_name))
                        $(.and_then(|chunks| chunks.parse::<$res>()))?
                }

                fields!(single_set $name => $field_name, $($res)?);
            }
        )*
    };

    (@type) => {&[Chunk]};
    (@type $res:ty) => {$res};
}

macro_rules! date_fields {
    ($($name:ident: $field_prefix:expr),* $(,)*) => {
        $(
            paste! {
                #[doc = "Get and parse the `" $field_prefix "date` field, falling back to the `" $field_prefix "year`, `" $field_prefix "month`, and `" $field_prefix "day` fields when not present."]
                pub fn [<get_ $name>](&self) -> anyhow::Result<Date> {
                    if let Some(chunks) = self.get(concat!($field_prefix, "date")) {
                        chunks.parse::<Date>()
                    } else {
                        Date::new_from_three_fields(
                            self.get(concat!($field_prefix, "year")),
                            self.get(concat!($field_prefix, "month")),
                            self.get(concat!($field_prefix, "day")),
                        )
                    }
                }

                #[doc = "Set a value in the `" $field_prefix "date` field."]
                pub fn [<set_ $name>](&mut self, item: Date) -> anyhow::Result<()> {
                    let chunks = item.to_chunks()?;
                    self.set(concat!($field_prefix, "date"), chunks);
                    self.delete(concat!($field_prefix, "year"));
                    self.delete(concat!($field_prefix, "month"));
                    self.delete(concat!($field_prefix, "day"));

                    Ok(())
                }
            }
        )*
    };
}

impl Entry {
    fields! {
        // Fields without a specified return type simply return `&[Chunk]`.
        // BibTeX fields.
        author: "author" => Vec<Person>,
        book_title: "booktitle",
        chapter: "chapter",
        edition: "edition" => IntOrChunks,
        how_published: "howpublished",
        note: "note",
        number: "number",
        organization: "organization" => Vec<Vec<Chunk>>,
        pages: "pages" => Vec<std::ops::Range<u32>>,
        publisher: "publisher" => Vec<Vec<Chunk>>,
        series: "series",
        title: "title",
        type: "type" => String,
        volume: "volume" => i64,
    }

    date_fields! {
        date: "",
        event_date: "event",
        orig_date: "orig",
        url_date: "url",
    }

    /// Get and parse the `editor` and `editora` through `editorc` fields
    /// and their respective `editortype` annotation fields.
    /// If any of the above fields is present, this function will return a
    /// vector with between one and four entries, one for each editorial role.
    ///
    /// The default `EditorType::Editor` will be assumed if the type field
    /// is empty.
    pub fn get_editors(&self) -> anyhow::Result<Vec<(Vec<Person>, EditorType)>> {
        let mut editors = vec![];

        let mut parse_editor_field = |name_field: &str, editor_field: &str| {
            self.get(name_field)
                .and_then(|chunks| chunks.parse::<Vec<Person>>().ok())
                .map(|persons| {
                    let editor_type = self
                        .get(editor_field)
                        .and_then(|chunks| chunks.parse::<EditorType>().ok())
                        .unwrap_or(EditorType::Editor);
                    editors.push((persons, editor_type));
                });
        };

        parse_editor_field("editor", "editortype");
        parse_editor_field("editora", "editoratype");
        parse_editor_field("editorb", "editorbtype");
        parse_editor_field("editorc", "editorctype");

        if editors.is_empty() {
            return Err(anyhow!("No editor fields present"));
        }

        Ok(editors)
    }

    alias_fields! {
        address: "address", "location",
        location: "location", "address",
        annotation: "annotation", "annote",
        eprint_type: "eprinttype", "archiveprefix",
        journal: "journal", "journaltitle",
        journal_title: "journaltitle", "journal",
        sort_key: "key", "sortkey" => String,
        file: "file", "pdf" => String,
        school: "school", "institution",
        institution: "institution", "school",
    }

    fields! {
        // BibLaTeX supplemental fields.
        abstract: "abstract",
        addendum: "addendum",
        afterword: "afterword" => Vec<Person>,
        annotator: "annotator" => Vec<Person>,
        author_type: "authortype" => String,
        book_author: "bookauthor" => Vec<Person>,
        book_pagination: "bookpagination" => Pagination,
        book_subtitle: "booksubtitle",
        book_title_addon: "booktitleaddon",
        commentator: "commentator" => Vec<Person>,
        doi: "doi" => String,
        eid: "eid",
        entry_subtype: "entrysubtype",
        eprint: "eprint" => String,
        eprint_class: "eprintclass",
        eventtitle: "eventtitle",
        eventtitle_addon: "eventtitleaddon",
        foreword: "foreword" => Vec<Person>,
        holder: "holder" => Vec<Person>,
        index_title: "indextitle",
        introduction: "introduction" => Vec<Person>,
        isan: "isan",
        isbn: "isbn",
        ismn: "ismn",
        isrn: "isrn",
        issn: "issn",
        issue: "issue",
        issue_subtitle: "issuesubtitle",
        issue_title: "issuetitle",
        issue_title_addon: "issuetitleaddon",
        iswc: "iswc",
        journal_subtitle: "journalsubtitle",
        journal_title_addon: "journaltitleaddon",
        keywords: "keywords",
        label: "label",
        language: "language" => String,
        library: "library",
        main_subtitle: "mainsubtitle",
        main_title: "maintitle",
        main_title_addon: "maintitleaddon",
        name_addon: "nameaddon",
        options: "options",
        orig_language: "origlanguage" => String,
        orig_location: "origlocation",
        page_total: "pagetotal",
        pagination: "pagination" => Pagination,
        part: "part",
        pubstate: "pubstate",
        reprint_title: "reprinttitle",
        short_author: "shortauthor" => Vec<Person>,
        short_editor: "shorteditor" => Vec<Person>,
        shorthand: "shorthand",
        shorthand_intro: "shorthandintro",
        short_journal: "shortjournal",
        short_series: "shortseries",
        short_title: "shorttitle",
        subtitle: "subtitle",
        title_addon: "titleaddon",
        translator: "translator" => Vec<Person>,
        url: "url" => String,
        venue: "venue",
        version: "version",
        volumes: "volumes" => i64,
        gender: "gender" => Gender,
    }
}

/// Additional methods for chunk slices.
pub trait ChunksExt {
    /// Parse the chunks into a type.
    fn parse<T: Type>(&self) -> anyhow::Result<T>;

    /// Format the chunks in sentence case.
    fn format_sentence(&self) -> String;

    /// Format the chunks verbatim.
    fn format_verbatim(&self) -> String;

    /// True when the chunk slice contains no non-empty chunk.
    fn is_empty(&self) -> bool;
}

impl ChunksExt for [Chunk] {
    fn parse<T: Type>(&self) -> anyhow::Result<T> {
        T::from_chunks(self)
    }

    fn format_sentence(&self) -> String {
        let mut out = String::new();
        let mut first = true;
        for val in self {
            if let Chunk::Normal(s) = val {
                for c in s.chars() {
                    if first {
                        out.push_str(&c.to_uppercase().to_string());
                    } else {
                        out.push(c);
                    }
                    first = false;
                }
            } else if let Chunk::Verbatim(s) = val {
                out += s;
            }
            first = false;
        }

        out
    }

    fn format_verbatim(&self) -> String {
        let mut out = String::new();
        for val in self {
            if let Chunk::Normal(s) = val {
                out += s;
            } else if let Chunk::Verbatim(s) = val {
                out += s;
            }
        }

        out
    }

    fn is_empty(&self) -> bool {
        self.iter().any(|c| match c {
            Chunk::Normal(s) => !s.is_empty(),
            Chunk::Verbatim(s) => !s.is_empty(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_gral_paper() {
        dump_debug("tests/gral.bib");
    }

    #[test]
    fn test_ds_report() {
        dump_debug("tests/ds.bib");
    }

    #[test]
    fn test_libra_paper() {
        dump_author_title("tests/libra.bib");
    }

    #[test]
    fn test_rass_report() {
        dump_author_title("tests/rass.bib");
    }

    #[test]
    fn test_alias() {
        let contents = fs::read_to_string("tests/cross.bib").unwrap();
        let mut bibliography = Bibliography::from_str(&contents, true);

        assert_eq!(bibliography.get("issue201"), bibliography.get("github"));
        bibliography.add_alias("issue201", "crap").expect("this must work");
        assert_eq!(bibliography.get("crap"), bibliography.get("unstable"));
        bibliography.remove_item("crap").expect("removal must work");
        let cf = bibliography.get("cannonfodder").unwrap();
        assert_eq!(cf.entry_type, EntryType::Misc);
        assert_eq!(cf.cite_key, "cannonfodder");
    }

    #[test]
    fn test_bibtex_conversion() {
        let contents = fs::read_to_string("tests/cross.bib").unwrap();
        let mut bibliography = Bibliography::from_str(&contents, true);

        let biblatex = bibliography.get_mut("haug2019").unwrap().as_biblatex_string();
        assert!(biblatex.contains("institution = {Technische Universität Berlin},"));

        let bibtex = bibliography.get_mut("haug2019").unwrap().as_bibtex_string();
        assert!(bibtex.contains("school = {Technische Universität Berlin},"));
        assert!(bibtex.contains("year = {2019},"));
        assert!(bibtex.contains("month = {10},"));
        assert!(!bibtex.contains("institution"));
        assert!(!bibtex.contains("date"));
    }

    #[test]
    fn test_verify() {
        let contents = fs::read_to_string("tests/cross.bib").unwrap();
        let mut bibliography = Bibliography::from_str(&contents, true);

        assert!(bibliography.get_mut("haug2019").unwrap().verify().is_ok());
        assert!(bibliography.get_mut("cannonfodder").unwrap().verify().is_ok());

        let ill = bibliography.get("ill-defined").unwrap();
        if let Err((exp, forb)) = ill.verify() {
            assert!(exp.contains(&"title"));
            assert!(exp.contains(&"year"));
            assert!(exp.contains(&"editor"));
            assert!(forb.contains(&"maintitle"));
            assert!(forb.contains(&"author"));
            assert!(forb.contains(&"chapter"));
            assert_eq!(exp.len(), 3);
            assert_eq!(forb.len(), 3);
        } else {
            panic!("Should not have passed test");
        }
    }

    #[test]
    fn test_crossref() {
        let contents = fs::read_to_string("tests/cross.bib").unwrap();
        let mut bibliography = Bibliography::from_str(&contents, true);

        let e = bibliography.get_resolved("macmillan").unwrap();
        assert_eq!(e.get_publisher().unwrap()[0].format_verbatim(), "Macmillan");
        assert_eq!(
            e.get_location().unwrap().format_verbatim(),
            "New York and London"
        );

        let book = bibliography.get_resolved("recursive").unwrap();
        assert_eq!(
            book.get_publisher().unwrap()[0].format_verbatim(),
            "Macmillan"
        );
        assert_eq!(
            book.get_location().unwrap().format_verbatim(),
            "New York and London"
        );
        assert_eq!(
            book.get_title().unwrap().format_verbatim(),
            "Recursive shennenigans and other important stuff"
        );

        assert_eq!(bibliography.get("arrgh").unwrap().get_parents(), vec![
            "polecon".to_string()
        ]);
        let arrgh = bibliography.get_resolved("arrgh").unwrap();
        assert_eq!(arrgh.entry_type, EntryType::Article);
        assert_eq!(arrgh.get_volume().unwrap(), 115);
        assert_eq!(arrgh.get_editors().unwrap()[0].0[0].name, "Uhlig");
        assert_eq!(arrgh.get_number().unwrap().format_verbatim(), "6");
        assert_eq!(
            arrgh.get_journal().unwrap().format_verbatim(),
            "Journal of Political Economy"
        );
        assert_eq!(
            arrgh.get_title().unwrap().format_verbatim(),
            "An‐arrgh‐chy: The Law and Economics of Pirate Organization"
        );
    }

    fn dump_debug(file: &str) {
        let contents = fs::read_to_string(file).unwrap();
        let bibliography = Bibliography::from_str(&contents, true);
        println!("{:#?}", bibliography);
    }

    fn dump_author_title(file: &str) {
        let contents = fs::read_to_string(file).unwrap();
        let mut bibliography = Bibliography::from_str(&contents, true);

        println!("{}", bibliography.as_biblatex_string());
        for x in bibliography {
            let authors = x.get_author().unwrap_or_default();
            for a in authors {
                print!("{}, ", a);
            }
            println!("\"{}\".", x.get_title().unwrap().format_sentence());
        }
    }
}
