//! Query builder and result types.

use hellodb_core::Record;

use crate::cursor::Cursor;
use crate::filter::Filter;
use crate::sort::SortField;

/// Hard ceiling on the page size of a single query, regardless of what the
/// caller asks for.
///
/// A retrieval tool that can be coaxed into returning millions of records
/// turns itself into a context-stuffing vector: the caller (or an LLM
/// driving the caller) sets `limit = 10_000_000` and gets a response that
/// swamps whatever window consumes it. The cap below closes that door at
/// the query layer so every read path inherits the same bound, not just
/// the MCP tool schemas.
///
/// 1000 is an order of magnitude above the default per-page size (100) —
/// large enough for legitimate bulk exports, small enough that it can't
/// singlehandedly fill a model context. Callers who genuinely need more
/// should paginate via the cursor.
pub const MAX_QUERY_LIMIT: usize = 1_000;

/// A query against hellodb records.
///
/// Use the builder pattern to construct:
/// ```ignore
/// Query::new()
///     .schema("app.commerce.listing")
///     .filter(Filter::Gt("price".into(), json!(20.0)))
///     .sort(SortField::desc("price"))
///     .limit(50)
/// ```
#[derive(Debug, Clone)]
pub struct Query {
    /// Only match records with this schema (None = all schemas).
    pub schema: Option<String>,
    /// Only match records in this namespace (None = all in scope).
    pub namespace: Option<String>,
    /// Filter predicate tree (None = no filtering).
    pub filter: Option<Filter>,
    /// Sort fields (applied in order). Empty = default order.
    pub sort: Vec<SortField>,
    /// Maximum records to return per page.
    pub limit: usize,
    /// Cursor-based pagination: resume after this cursor.
    pub after: Option<Cursor>,
    /// Offset-based pagination fallback (0 = disabled).
    pub offset: usize,
}

impl Query {
    /// Create a new query with defaults (limit=100, no filters).
    pub fn new() -> Self {
        Self {
            schema: None,
            namespace: None,
            filter: None,
            sort: Vec::new(),
            limit: 100,
            after: None,
            offset: 0,
        }
    }

    /// Filter by schema ID.
    pub fn schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    /// Filter by namespace.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set the filter predicate.
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Add a sort field.
    pub fn sort(mut self, field: SortField) -> Self {
        self.sort.push(field);
        self
    }

    /// Set page size limit. Silently clamped to `MAX_QUERY_LIMIT` —
    /// callers asking for more get the cap. This is the single chokepoint
    /// for all read paths that go through the query engine, so no downstream
    /// tool can accidentally return an unbounded page by plumbing a large
    /// integer through.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit.min(MAX_QUERY_LIMIT);
        self
    }

    /// Set cursor for pagination.
    pub fn after(mut self, cursor: Cursor) -> Self {
        self.after = Some(cursor);
        self
    }

    /// Set offset for offset-based pagination.
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
}

impl Default for Query {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of executing a query.
#[derive(Debug)]
pub struct QueryResult {
    /// Matching records for this page.
    pub records: Vec<Record>,
    /// Total count of matching records (across all pages).
    pub total_count: u64,
    /// Cursor to fetch the next page (None if no more results).
    pub next_cursor: Option<Cursor>,
    /// Whether there are more results beyond this page.
    pub has_more: bool,
}
