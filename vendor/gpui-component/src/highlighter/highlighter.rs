use crate::highlighter::{HighlightTheme, LanguageRegistry};
use crate::input::RopeExt;

use anyhow::{anyhow, Context, Result};
use gpui::{HighlightStyle, SharedString};

use ropey::{ChunkCursor, Rope};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    ops::Range,
    usize,
};
use sum_tree::Bias;
use tree_sitter::{
    InputEdit, Node, Parser, Point, Query, QueryCursor, QueryMatch, StreamingIterator, Tree,
};

/// A syntax highlighter that supports incremental parsing, multiline text,
/// and caching of highlight results.
#[allow(unused)]
pub struct SyntaxHighlighter {
    language: SharedString,
    query: Option<Query>,
    injection_queries: HashMap<SharedString, Query>,

    locals_pattern_index: usize,
    highlights_pattern_index: usize,
    // highlight_indices: Vec<Option<Highlight>>,
    non_local_variable_patterns: Vec<bool>,
    injection_content_capture_index: Option<u32>,
    injection_language_capture_index: Option<u32>,
    local_scope_capture_index: Option<u32>,
    local_def_capture_index: Option<u32>,
    local_def_value_capture_index: Option<u32>,
    local_ref_capture_index: Option<u32>,

    /// The last parsed source text.
    text: Rope,
    parser: Parser,
    /// The last parsed tree.
    tree: Option<Tree>,

    /// [Branchpad] Bumped whenever the parsed text changes; keys `styles_cache`.
    version: u64,
    /// [Branchpad] Last `styles()` result, keyed by (range, version, theme address), so
    /// frames that don't scroll or edit (cursor blink, scrollbar fade) skip the tree-sitter query.
    styles_cache: RefCell<Option<StylesCache>>,
}

struct StylesCache {
    range: Range<usize>,
    version: u64,
    theme: usize,
    styles: Vec<(Range<usize>, HighlightStyle)>,
}

struct TextProvider<'a>(&'a Rope);
struct ByteChunks<'a> {
    cursor: ChunkCursor<'a>,
    end: usize,
}
impl<'a> tree_sitter::TextProvider<&'a [u8]> for TextProvider<'a> {
    type I = ByteChunks<'a>;

    fn text(&mut self, node: tree_sitter::Node) -> Self::I {
        let range = node.byte_range();
        let cursor = self.0.chunk_cursor_at(range.start);

        ByteChunks {
            cursor,
            end: range.end,
        }
    }
}

impl<'a> Iterator for ByteChunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let cursor = &mut self.cursor;
        let end = self.end;

        if cursor.next() && cursor.byte_offset() < end {
            Some(cursor.chunk().as_bytes())
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Clone)]
struct HighlightSummary {
    count: usize,
    start: usize,
    end: usize,
    min_start: usize,
    max_end: usize,
}

/// The highlight item, the range is offset of the token in the tree.
#[derive(Debug, Default, Clone)]
struct HighlightItem {
    /// The byte range of the highlight in the text.
    range: Range<usize>,
    /// The highlight name, like `function`, `string`, `comment`, etc.
    name: SharedString,
}

impl HighlightItem {
    pub fn new(range: Range<usize>, name: impl Into<SharedString>) -> Self {
        Self {
            range,
            name: name.into(),
        }
    }
}

impl sum_tree::Item for HighlightItem {
    type Summary = HighlightSummary;
    fn summary(&self, _cx: &()) -> Self::Summary {
        HighlightSummary {
            count: 1,
            start: self.range.start,
            end: self.range.end,
            min_start: self.range.start,
            max_end: self.range.end,
        }
    }
}

impl sum_tree::Summary for HighlightSummary {
    type Context<'a> = &'a ();
    fn zero(_: Self::Context<'_>) -> Self {
        HighlightSummary {
            count: 0,
            start: usize::MIN,
            end: usize::MAX,
            min_start: usize::MAX,
            max_end: usize::MIN,
        }
    }

    fn add_summary(&mut self, other: &Self, _: Self::Context<'_>) {
        self.min_start = self.min_start.min(other.min_start);
        self.max_end = self.max_end.max(other.max_end);
        self.start = other.start;
        self.end = other.end;
        self.count += other.count;
    }
}

impl<'a> sum_tree::Dimension<'a, HighlightSummary> for usize {
    fn zero(_: &()) -> Self {
        0
    }

    fn add_summary(&mut self, _: &'a HighlightSummary, _: &()) {}
}

impl<'a> sum_tree::Dimension<'a, HighlightSummary> for Range<usize> {
    fn zero(_: &()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a HighlightSummary, _: &()) {
        self.start = summary.start;
        self.end = summary.end;
    }
}

impl SyntaxHighlighter {
    /// Create a new SyntaxHighlighter for HTML.
    pub fn new(lang: &str) -> Self {
        match Self::build_combined_injections_query(&lang) {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!(
                    "SyntaxHighlighter init failed, fallback to use `text`, {}",
                    err
                );
                Self::build_combined_injections_query("text").unwrap()
            }
        }
    }

    /// Build the combined injections query for the given language.
    ///
    /// https://github.com/tree-sitter/tree-sitter/blob/v0.25.5/highlight/src/lib.rs#L336
    fn build_combined_injections_query(lang: &str) -> Result<Self> {
        let Some(config) = LanguageRegistry::singleton().language(&lang) else {
            return Err(anyhow!(
                "language {:?} is not registered in `LanguageRegistry`",
                lang
            ));
        };

        let mut parser = Parser::new();
        parser
            .set_language(&config.language)
            .context("parse set_language")?;

        // Concatenate the query strings, keeping track of the start offset of each section.
        let mut query_source = String::new();
        query_source.push_str(&config.injections);
        let locals_query_offset = query_source.len();
        query_source.push_str(&config.locals);
        let highlights_query_offset = query_source.len();
        query_source.push_str(&config.highlights);

        // Construct a single query by concatenating the three query strings, but record the
        // range of pattern indices that belong to each individual string.
        let query = Query::new(&config.language, &query_source).context("new query")?;

        let mut locals_pattern_index = 0;
        let mut highlights_pattern_index = 0;
        for i in 0..(query.pattern_count()) {
            let pattern_offset = query.start_byte_for_pattern(i);
            if pattern_offset < highlights_query_offset {
                if pattern_offset < highlights_query_offset {
                    highlights_pattern_index += 1;
                }
                if pattern_offset < locals_query_offset {
                    locals_pattern_index += 1;
                }
            }
        }

        // let Some(mut combined_injections_query) =
        //     Query::new(&config.language, &config.injections).ok()
        // else {
        //     return None;
        // };

        // let mut has_combined_queries = false;
        // for pattern_index in 0..locals_pattern_index {
        //     let settings = query.property_settings(pattern_index);
        //     if settings.iter().any(|s| &*s.key == "injection.combined") {
        //         has_combined_queries = true;
        //         query.disable_pattern(pattern_index);
        //     } else {
        //         combined_injections_query.disable_pattern(pattern_index);
        //     }
        // }
        // let combined_injections_query = if has_combined_queries {
        //     Some(combined_injections_query)
        // } else {
        //     None
        // };

        // Find all of the highlighting patterns that are disabled for nodes that
        // have been identified as local variables.
        let non_local_variable_patterns = (0..query.pattern_count())
            .map(|i| {
                query
                    .property_predicates(i)
                    .iter()
                    .any(|(prop, positive)| !*positive && prop.key.as_ref() == "local")
            })
            .collect();

        // Store the numeric ids for all of the special captures.
        let mut injection_content_capture_index = None;
        let mut injection_language_capture_index = None;
        let mut local_def_capture_index = None;
        let mut local_def_value_capture_index = None;
        let mut local_ref_capture_index = None;
        let mut local_scope_capture_index = None;
        for (i, name) in query.capture_names().iter().enumerate() {
            let i = Some(i as u32);
            match *name {
                "injection.content" => injection_content_capture_index = i,
                "injection.language" => injection_language_capture_index = i,
                "local.definition" => local_def_capture_index = i,
                "local.definition-value" => local_def_value_capture_index = i,
                "local.reference" => local_ref_capture_index = i,
                "local.scope" => local_scope_capture_index = i,
                _ => {}
            }
        }

        let mut injection_queries = HashMap::new();
        for inj_language in config.injection_languages.iter() {
            if let Some(inj_config) = LanguageRegistry::singleton().language(&inj_language) {
                match Query::new(&inj_config.language, &inj_config.highlights) {
                    Ok(q) => {
                        injection_queries.insert(inj_config.name.clone(), q);
                    }
                    Err(e) => {
                        tracing::error!(
                            "failed to build injection query for {:?}: {:?}",
                            inj_config.name,
                            e
                        );
                    }
                }
            }
        }

        // let highlight_indices = vec![None; query.capture_names().len()];

        Ok(Self {
            language: config.name.clone(),
            query: Some(query),
            injection_queries,

            locals_pattern_index,
            highlights_pattern_index,
            non_local_variable_patterns,
            injection_content_capture_index,
            injection_language_capture_index,
            local_scope_capture_index,
            local_def_capture_index,
            local_def_value_capture_index,
            local_ref_capture_index,
            text: Rope::new(),
            parser,
            tree: None,
            version: 0,
            styles_cache: RefCell::new(None),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.text.len() == 0
    }

    /// Highlight the given text, returning a map from byte ranges to highlight captures.
    ///
    /// Uses incremental parsing by `edit` to efficiently update the highlighter's state.
    pub fn update(&mut self, edit: Option<InputEdit>, text: &Rope) {
        if self.text.eq(text) {
            return;
        }

        let edit = edit.unwrap_or(InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: text.len(),
            start_position: Point::new(0, 0),
            old_end_position: Point::new(0, 0),
            new_end_position: Point::new(0, 0),
        });

        let mut old_tree = self
            .tree
            .take()
            .unwrap_or(self.parser.parse("", None).unwrap());
        old_tree.edit(&edit);

        let new_tree = self.parser.parse_with_options(
            &mut move |offset, _| {
                if offset >= text.len() {
                    ""
                } else {
                    let (chunk, chunk_byte_ix) = text.chunk(offset);
                    &chunk[offset - chunk_byte_ix..]
                }
            },
            Some(&old_tree),
            None,
        );

        let Some(new_tree) = new_tree else {
            return;
        };

        self.tree = Some(new_tree);
        self.text = text.clone();
        self.version = self.version.wrapping_add(1);
        *self.styles_cache.get_mut() = None;
    }

    /// Match the visible ranges of nodes in the Tree for highlighting.
    fn match_styles(&self, range: Range<usize>) -> Vec<HighlightItem> {
        let mut highlights = vec![];
        let Some(tree) = &self.tree else {
            return highlights;
        };

        let Some(query) = &self.query else {
            return highlights;
        };

        let root_node = tree.root_node();

        let source = &self.text;
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let mut matches = cursor.matches(&query, root_node, TextProvider(&source));

        while let Some(query_match) = matches.next() {
            // Ref:
            // https://github.com/tree-sitter/tree-sitter/blob/460118b4c82318b083b4d527c9c750426730f9c0/highlight/src/lib.rs#L556
            if let (Some(language_name), Some(content_node), _) =
                self.injection_for_match(None, query, query_match)
            {
                let styles = self.handle_injection(&language_name, content_node);
                for (node_range, highlight_name) in styles {
                    highlights.push(HighlightItem::new(node_range.clone(), highlight_name));
                }

                continue;
            }

            for cap in query_match.captures {
                let node = cap.node;

                let Some(highlight_name) = query.capture_names().get(cap.index as usize) else {
                    continue;
                };

                let node_range: Range<usize> = node.start_byte()..node.end_byte();
                let highlight_name = SharedString::from(highlight_name.to_string());

                // Merge near range and same highlight name
                let last_item = highlights.last();
                let last_range = last_item.map(|item| &item.range).unwrap_or(&(0..0));
                let last_highlight_name = last_item.map(|item| item.name.clone());

                if last_range.end <= node_range.start
                    && last_highlight_name.as_ref() == Some(&highlight_name)
                {
                    highlights.push(HighlightItem::new(
                        last_range.start..node_range.end,
                        highlight_name.clone(),
                    ));
                } else if last_range == &node_range {
                    // case:
                    // last_range: 213..220, last_highlight_name: Some("property")
                    // last_range: 213..220, last_highlight_name: Some("string")
                    highlights.push(HighlightItem::new(
                        node_range,
                        last_highlight_name.unwrap_or(highlight_name),
                    ));
                } else {
                    highlights.push(HighlightItem::new(node_range, highlight_name.clone()));
                }
            }
        }

        // DO NOT REMOVE THIS PRINT, it's useful for debugging
        // for item in highlights {
        //     println!("item: {:?}", item);
        // }

        highlights
    }

    /// TODO: Use incremental parsing to handle the injection.
    fn handle_injection(
        &self,
        injection_language: &str,
        node: Node,
    ) -> Vec<(Range<usize>, String)> {
        // Ensure byte offsets are on char boundaries for UTF-8 safety
        let start_offset = self.text.clip_offset(node.start_byte(), Bias::Left);
        let end_offset = self.text.clip_offset(node.end_byte(), Bias::Right);

        let mut cache = vec![];
        let Some(query) = &self.injection_queries.get(injection_language) else {
            return cache;
        };

        let content = self.text.slice(start_offset..end_offset);
        if content.len() == 0 {
            return cache;
        };
        // FIXME: Avoid to_string.
        let content = content.to_string();

        let Some(config) = LanguageRegistry::singleton().language(injection_language) else {
            return cache;
        };
        let mut parser = Parser::new();
        if parser.set_language(&config.language).is_err() {
            return cache;
        }

        let source = content.as_bytes();
        let Some(tree) = parser.parse(source, None) else {
            return cache;
        };

        let mut query_cursor = QueryCursor::new();
        let mut matches = query_cursor.matches(query, tree.root_node(), source);

        let mut last_end = start_offset;
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let cap_node = cap.node;

                let node_range: Range<usize> =
                    start_offset + cap_node.start_byte()..start_offset + cap_node.end_byte();

                if node_range.start < last_end {
                    continue;
                }
                if node_range.end > end_offset {
                    break;
                }

                if let Some(highlight_name) = query.capture_names().get(cap.index as usize) {
                    last_end = node_range.end;
                    cache.push((node_range, highlight_name.to_string()));
                }
            }
        }

        cache
    }

    /// Ref:
    /// https://github.com/tree-sitter/tree-sitter/blob/v0.25.5/highlight/src/lib.rs#L1229
    ///
    /// Returns:
    /// - `language_name`: The language name of the injection.
    /// - `content_node`: The content node of the injection.
    /// - `include_children`: Whether to include the children of the content node.
    fn injection_for_match<'a>(
        &self,
        parent_name: Option<SharedString>,
        query: &'a Query,
        query_match: &QueryMatch<'a, 'a>,
    ) -> (Option<SharedString>, Option<Node<'a>>, bool) {
        let content_capture_index = self.injection_content_capture_index;
        // let language_capture_index = self.injection_language_capture_index;

        let mut language_name: Option<SharedString> = None;
        let mut content_node = None;

        for capture in query_match.captures {
            let index = Some(capture.index);
            if index == content_capture_index {
                content_node = Some(capture.node);
            }
        }

        let mut include_children = false;
        for prop in query.property_settings(query_match.pattern_index) {
            match prop.key.as_ref() {
                // In addition to specifying the language name via the text of a
                // captured node, it can also be hard-coded via a `#set!` predicate
                // that sets the injection.language key.
                "injection.language" => {
                    if language_name.is_none() {
                        language_name = prop
                            .value
                            .as_ref()
                            .map(std::convert::AsRef::as_ref)
                            .map(ToString::to_string)
                            .map(SharedString::from);
                    }
                }

                // Setting the `injection.self` key can be used to specify that the
                // language name should be the same as the language of the current
                // layer.
                "injection.self" => {
                    if language_name.is_none() {
                        language_name = Some(self.language.clone());
                    }
                }

                // Setting the `injection.parent` key can be used to specify that
                // the language name should be the same as the language of the
                // parent layer
                "injection.parent" => {
                    if language_name.is_none() {
                        language_name = parent_name.clone();
                    }
                }

                // By default, injections do not include the *children* of an
                // `injection.content` node - only the ranges that belong to the
                // node itself. This can be changed using a `#set!` predicate that
                // sets the `injection.include-children` key.
                "injection.include-children" => include_children = true,
                _ => {}
            }
        }

        (language_name, content_node, include_children)
    }

    /// Returns the syntax highlight styles for a range of text.
    ///
    /// The argument `range` is the range of bytes in the text to highlight.
    ///
    /// Returns a vector of tuples where each tuple contains:
    /// - A byte range relative to the text
    /// - The corresponding highlight style for that range
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
    /// use ropey::Rope;
    ///
    /// let code = "fn main() {\n    println!(\"Hello\");\n}";
    /// let rope = Rope::from_str(code);
    /// let mut highlighter = SyntaxHighlighter::new("rust");
    /// highlighter.update(None, &rope);
    ///
    /// let theme = HighlightTheme::default_dark();
    /// let range = 0..code.len();
    /// let styles = highlighter.styles(&range, &theme);
    /// ```
    pub fn styles(
        &self,
        range: &Range<usize>,
        theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let theme_key = theme as *const HighlightTheme as usize;
        if let Some(c) = &*self.styles_cache.borrow() {
            if c.version == self.version && c.theme == theme_key && c.range == *range {
                return c.styles.clone();
            }
        }
        let styles = self.compute_styles(range, theme);
        *self.styles_cache.borrow_mut() = Some(StylesCache {
            range: range.clone(),
            version: self.version,
            theme: theme_key,
            styles: styles.clone(),
        });
        styles
    }

    fn compute_styles(
        &self,
        range: &Range<usize>,
        theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut styles = vec![];
        let start_offset = range.start;

        let highlights = self.match_styles(range.clone());

        // let mut iter_count = 0;
        for item in highlights {
            // iter_count += 1;
            let node_range = &item.range;
            let name = &item.name;

            // Avoid start larger than end
            let mut node_range = node_range.start.max(range.start)..node_range.end.min(range.end);
            if node_range.start > node_range.end {
                node_range.end = node_range.start;
            }

            styles.push((node_range, theme.style(name.as_ref()).unwrap_or_default()));
        }

        // If the matched styles is empty, return a default range.
        if styles.len() == 0 {
            return vec![(start_offset..range.end, HighlightStyle::default())];
        }

        let styles = unique_styles(&range, styles);

        // NOTE: DO NOT remove this comment, it is used for debugging.
        // for style in &styles {
        //     println!("---- style: {:?} - {:?}", style.0, style.1.color);
        // }
        // println!("--------------------------------");

        styles
    }
}

/// To merge intersection ranges, let the subsequent range cover
/// the previous overlapping range and split the previous range.
///
/// From:
///
/// AA
///   BBB
///    CCCCC
///      DD
///         EEEE
///
/// To:
///
/// AABCCDDCEEEE
pub(crate) fn unique_styles(
    total_range: &Range<usize>,
    styles: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if styles.is_empty() {
        return styles;
    }

    // [Branchpad] Sweep instead of "for every interval scan every style" (O(intervals x styles), millions of
    // comparisons per frame once the visible block holds thousands of captures). Same result: for each
    // interval the styles covering it are merged in their original order.
    //
    // For example
    //
    // from: [(6..11), (6..11), (11..17), (17..25), (16..19), (25..59))]
    // to:   [6, 11, 16, 17, 19, 25, 59]
    let mut points: Vec<usize> = Vec::with_capacity(styles.len() * 2 + 2);
    points.push(total_range.start);
    points.push(total_range.end);
    // End points are significant for merging decisions.
    let mut significant: Vec<usize> = Vec::with_capacity(styles.len());
    for (range, _) in &styles {
        points.push(range.start);
        points.push(range.end);
        significant.push(range.end);
    }
    points.sort_unstable();
    points.dedup();
    significant.sort_unstable();
    significant.dedup();

    // Style indexes ordered by start; `active` keeps the covering ones ordered by index (push order).
    let mut by_start: Vec<usize> = (0..styles.len()).filter(|&i| styles[i].0.start < styles[i].0.end).collect();
    by_start.sort_by_key(|&i| styles[i].0.start);
    let mut next_start = 0;
    let mut active: BTreeSet<usize> = BTreeSet::new();

    // For each interval between boundaries, find the top-most style
    //
    // Result e.g.:
    //
    // [(6..11, red), (11..16, green), (16..17, blue), (17..19, red), (19..25, clean), (25..59, blue)]
    let mut result = Vec::with_capacity(points.len().saturating_sub(1));
    for w in points.windows(2) {
        let interval = w[0]..w[1];
        // `points` is deduplicated, so every interval is non-empty.
        while next_start < by_start.len() && styles[by_start[next_start]].0.start <= interval.start {
            active.insert(by_start[next_start]);
            next_start += 1;
        }
        active.retain(|&i| styles[i].0.end > interval.start);

        // Every range end is a point, so a range still active here covers the whole interval.
        let mut top_style: Option<HighlightStyle> = None;
        for &i in &active {
            match &mut top_style {
                Some(top) => merge_highlight_style(top, &styles[i].1),
                None => top_style = Some(styles[i].1),
            }
        }
        result.push((interval, top_style.unwrap_or_default()));
    }

    // Merge adjacent ranges with the same style, but not across significant boundaries
    let mut merged: Vec<(Range<usize>, HighlightStyle)> = Vec::with_capacity(result.len());
    for (range, style) in result {
        if let Some((last_range, last_style)) = merged.last_mut() {
            if last_range.end == range.start
                && *last_style == style
                && significant.binary_search(&range.start).is_err()
            {
                // Merge adjacent ranges with same style, but not across significant boundaries
                last_range.end = range.end;
                continue;
            }
        }
        merged.push((range, style));
    }

    merged
}

/// Merge other style (Other on top)
fn merge_highlight_style(style: &mut HighlightStyle, other: &HighlightStyle) {
    if let Some(color) = other.color {
        style.color = Some(color);
    }
    if let Some(font_weight) = other.font_weight {
        style.font_weight = Some(font_weight);
    }
    if let Some(font_style) = other.font_style {
        style.font_style = Some(font_style);
    }
    if let Some(background_color) = other.background_color {
        style.background_color = Some(background_color);
    }
    if let Some(underline) = other.underline {
        style.underline = Some(underline);
    }
    if let Some(strikethrough) = other.strikethrough {
        style.strikethrough = Some(strikethrough);
    }
    if let Some(fade_out) = other.fade_out {
        style.fade_out = Some(fade_out);
    }
}

#[cfg(test)]
mod tests {
    use gpui::Hsla;

    use super::*;
    use crate::Colorize as _;

    fn color_style(color: Hsla) -> HighlightStyle {
        let mut style = HighlightStyle::default();
        style.color = Some(color);
        style
    }

    #[track_caller]
    fn assert_unique_styles(
        range: Range<usize>,
        left: Vec<(Range<usize>, HighlightStyle)>,
        right: Vec<(Range<usize>, HighlightStyle)>,
    ) {
        fn color_name(c: Option<Hsla>) -> String {
            match c {
                Some(c) => {
                    if c == gpui::red() {
                        "red".to_string()
                    } else if c == gpui::green() {
                        "green".to_string()
                    } else if c == gpui::blue() {
                        "blue".to_string()
                    } else {
                        c.to_hex()
                    }
                }
                None => "clean".to_string(),
            }
        }

        let left = unique_styles(&range, left);
        if left.len() != right.len() {
            println!("\n---------------------------------------------");
            for (range, style) in left.iter() {
                println!("({:?}, {})", range, color_name(style.color));
            }
            println!("---------------------------------------------");
            panic!("left {} styles, right {} styles", left.len(), right.len());
        }
        for (left, right) in left.into_iter().zip(right) {
            if left.1.color != right.1.color || left.0 != right.0 {
                panic!(
                    "\n left: ({:?}, {})\nright: ({:?}, {})\n",
                    left.0,
                    color_name(left.1.color),
                    right.0,
                    color_name(right.1.color)
                );
            }
        }
    }

    #[test]
    fn test_unique_styles() {
        let red = color_style(gpui::red());
        let green = color_style(gpui::green());
        let blue = color_style(gpui::blue());
        let clean = HighlightStyle::default();

        assert_unique_styles(
            0..65,
            vec![
                (2..10, clean),
                (2..10, clean),
                (5..11, red),
                (2..6, clean),
                (10..15, green),
                (15..30, clean),
                (29..35, blue),
                (35..40, green),
                (45..60, blue),
            ],
            vec![
                (0..5, clean),
                (5..6, red),
                (6..10, red),
                (10..11, green),
                (11..15, green),
                (15..29, clean),
                (29..30, blue),
                (30..35, blue),
                (35..40, green),
                (40..45, clean),
                (45..60, blue),
                (60..65, clean),
            ],
        );
    }

    /// The original O(intervals x styles) implementation, kept as the reference for the sweep.
    fn naive_unique_styles(
        total_range: &Range<usize>,
        styles: &[(Range<usize>, HighlightStyle)],
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut intervals = BTreeSet::new();
        let mut significant = BTreeSet::new();
        intervals.insert(total_range.start);
        intervals.insert(total_range.end);
        for (range, _) in styles {
            intervals.insert(range.start);
            intervals.insert(range.end);
            significant.insert(range.end);
        }
        let intervals: Vec<usize> = intervals.into_iter().collect();
        let mut result = vec![];
        for i in 0..intervals.len().saturating_sub(1) {
            let interval = intervals[i]..intervals[i + 1];
            if interval.start >= interval.end {
                continue;
            }
            let mut top: Option<HighlightStyle> = None;
            for (range, style) in styles {
                if range.start <= interval.start && interval.end <= range.end {
                    match &mut top {
                        Some(t) => merge_highlight_style(t, style),
                        None => top = Some(*style),
                    }
                }
            }
            result.push((interval, top.unwrap_or_default()));
        }
        let mut merged: Vec<(Range<usize>, HighlightStyle)> = vec![];
        for (range, style) in result {
            if let Some((last_range, last_style)) = merged.last_mut() {
                if last_range.end == range.start
                    && *last_style == style
                    && !significant.contains(&range.start)
                {
                    last_range.end = range.end;
                    continue;
                }
            }
            merged.push((range, style));
        }
        merged
    }

    #[test]
    fn unique_styles_matches_naive() {
        let palette = [
            HighlightStyle::default(),
            color_style(gpui::red()),
            color_style(gpui::green()),
            color_style(gpui::blue()),
        ];
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut next = move |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        for _ in 0..500 {
            let total = 0..(20 + next(80));
            let count = 1 + next(40);
            let styles: Vec<(Range<usize>, HighlightStyle)> = (0..count)
                .map(|_| {
                    let start = next(total.end + 5);
                    let len = next(30); // includes empty ranges
                    (start..start + len, palette[next(palette.len())])
                })
                .collect();
            let expected = naive_unique_styles(&total, &styles);
            let actual = unique_styles(&total, styles.clone());
            assert_eq!(expected.len(), actual.len(), "styles: {styles:?}");
            for (e, a) in expected.iter().zip(&actual) {
                assert!(e.0 == a.0 && e.1 == a.1, "styles: {styles:?}\nexpected {e:?}\nactual {a:?}");
            }
        }
    }
}
