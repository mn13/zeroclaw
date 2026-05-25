/// Describes a single secret field discovered via `#[derive(Configurable)]`.
#[derive(Debug, Clone)]
pub struct SecretFieldInfo {
    /// Full dotted name (e.g. `channels.matrix.access-token`)
    pub name: &'static str,
    /// Category for grouping in `zeroclaw config list`
    pub category: &'static str,
    /// Whether this field currently has a non-empty value
    pub is_set: bool,
}

/// Runtime type classification for config property values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    String,
    Bool,
    Integer,
    Float,
    /// An enum or other serde-serializable type (parsed as TOML string).
    Enum,
    /// A `Vec<String>` field; set via comma-separated input.
    StringArray,
    /// A `Vec<T>` field where `T` is a serializable struct (e.g. `Vec<McpServerConfig>`,
    /// `Vec<PeripheralBoardConfig>`). Round-tripped on the wire as a JSON array of
    /// objects; the dashboard renders a per-row sub-form using the JSON Schema
    /// from `OPTIONS /api/config` to discover the element type's field shape.
    /// Schema v3 / #5947 will migrate the load-bearing ones (mcp.servers etc.)
    /// to `HashMap<String, T>` keyed tables; until then this kind covers them.
    ObjectArray,
    /// A struct-shaped scalar field (e.g. `Option<ModelPricing>`). Round-tripped
    /// on the wire as a JSON object; the dashboard renders a sub-form for the
    /// inner fields using the JSON Schema from `OPTIONS /api/config`. Distinct
    /// from `String`, which inserts the raw value as a TOML string and breaks
    /// the serde round-trip for typed structs (#6357 review).
    Object,
}

/// Maps Rust types to PropKind at compile time.
/// Scalars have explicit impls; the blanket impl catches everything
/// else as `PropKind::Enum`.
pub trait HasPropKind {
    const PROP_KIND: PropKind;
}

macro_rules! impl_prop_kind {
    ($kind:expr, $($ty:ty),+) => {
        $(impl HasPropKind for $ty { const PROP_KIND: PropKind = $kind; })+
    };
}

impl_prop_kind!(PropKind::Bool, bool);
impl_prop_kind!(PropKind::String, String);
impl_prop_kind!(PropKind::Float, f64, f32);
impl_prop_kind!(
    PropKind::Integer,
    u8,
    u16,
    u32,
    u64,
    usize,
    i8,
    i16,
    i32,
    i64,
    isize
);
impl HasPropKind for Vec<String> {
    const PROP_KIND: PropKind = PropKind::StringArray;
}

/// Describes a single property field discovered via `#[derive(Configurable)]`.
#[derive(Clone)]
pub struct PropFieldInfo {
    /// Full dotted name (e.g. `channels.telegram.draft-update-interval-ms`).
    /// Owned so the `HashMap<String, T>` branch of the derive can inject the
    /// runtime map key into the path (`providers.models.anthropic.api-key`)
    /// — `&'static str` can't carry user-supplied keys.
    pub name: String,
    /// Category for grouping in property listings
    pub category: &'static str,
    /// Current value formatted for display (secrets show `"****"`)
    pub display_value: String,
    /// Raw Rust type string for display (e.g. `"bool"`, `"u64"`, `"Option<StreamMode>"`)
    pub type_hint: &'static str,
    /// Runtime type classification
    pub kind: PropKind,
    /// Whether this field is marked `#[secret]`
    pub is_secret: bool,
    /// Returns valid variant names for enum fields (None for non-enum fields)
    pub enum_variants: Option<fn() -> Vec<String>>,
    /// Field's `///` doc comment, flattened to a single line. Empty string
    /// when the field has no doc comment. Onboard uses this as human-readable
    /// prompt text instead of the raw kebab-case field name.
    pub description: &'static str,
    /// Whether this field's value is derived from a secret (`#[derived_from_secret]`).
    /// Subject to the same write-only / no-readback rules as `#[secret]`.
    /// Reserved for future schema additions; currently no fields are derived.
    pub derived_from_secret: bool,
}

impl PropFieldInfo {
    pub fn is_enum(&self) -> bool {
        self.enum_variants.is_some()
    }
}

impl std::fmt::Debug for PropFieldInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropFieldInfo")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("is_secret", &self.is_secret)
            .finish_non_exhaustive()
    }
}

/// Stable wire-form for an addable section — a `HashMap<String, T>` (Map) or
/// `Vec<T>` (List) field whose value type implements `Configurable`. The
/// dashboard / CLI use this to surface `+ Add` affordances without
/// hardcoding the section list. Auto-discovered by the `Configurable` derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "schema-export",
    derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)
)]
#[cfg_attr(feature = "schema-export", serde(rename_all = "snake_case"))]
pub enum MapKeyKind {
    /// `HashMap<String, T>` — key is user-supplied; new value is default.
    Map,
    /// `Vec<T>` — entries are appended; the user-supplied "key" is stored
    /// in the value type's natural identifier field (e.g. `name`, `hint`).
    List,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "schema-export",
    derive(serde::Serialize, schemars::JsonSchema)
)]
pub struct MapKeySection {
    /// Dotted section path, e.g. `providers.models`, `mcp.servers`.
    pub path: &'static str,
    /// Whether the section is a map or a list.
    pub kind: MapKeyKind,
    /// Rust type name of the value, e.g. `ModelProviderConfig`. For display only.
    pub value_type: &'static str,
    /// Doc comment on the field (flattened to one line). What the user sees
    /// when picking which kind of thing to add.
    pub description: &'static str,
}

/// One row emitted by the `Configurable` derive's `nested_option_entries()`
/// method — every `#[nested] Option<XConfig>` field on a struct shows up here
/// with its `present` bit and the per-field `#[display_name = "..."]` /
/// `#[description = "..."]` metadata. The integrations registry consumes
/// this verbatim instead of carrying its own per-field hand-list.
#[derive(Debug, Clone, Copy)]
pub struct NestedOptionEntry {
    /// snake_case field name on the parent struct (e.g. `"telegram"`,
    /// `"voice_duplex"`).
    pub field: &'static str,
    /// `true` when the parent struct's field is `Some(_)`.
    pub present: bool,
    /// Display name from `#[display_name = "..."]`; falls back to a
    /// title-cased rendering of the snake_case field name when the
    /// attribute is absent.
    pub display_name: &'static str,
    /// One-line summary from `#[description = "..."]`. Empty when the
    /// attribute is absent.
    pub description: &'static str,
}

/// One row emitted by the `Configurable` derive's `integration_descriptor()`
/// method on structs annotated with `#[integration(...)]`. Used for nested
/// toggleable configs (e.g. `BrowserConfig`, `CronConfig`) where the
/// integration is "active" iff a named bool field on the struct is `true`.
#[derive(Debug, Clone, Copy)]
pub struct IntegrationDescriptor {
    pub display_name: &'static str,
    pub description: &'static str,
    /// Free-form category label (e.g. `"ToolsAutomation"`). The
    /// integrations registry maps this string to its own
    /// `IntegrationCategory` enum so the schema crate doesn't have to
    /// depend on it.
    pub category: &'static str,
    /// Snapshot of the named status field at the moment this descriptor
    /// was built (`status_field = "enabled"` ⇒ `self.enabled`).
    pub active: bool,
}

/// Per-field secret operations the `Configurable` derive emits for every
/// `#[secret]` field. Generalizes encrypt / decrypt / is_set across the
/// supported shapes — `String`, `Option<String>`, `Vec<String>`,
/// `HashMap<String, String>`, and `Option<HashMap<String, String>>` — so
/// adding a new shape is a single trait impl rather than another branch
/// in the macro.
///
/// `encrypt_in_place` and `decrypt_in_place` are idempotent: encrypting an
/// already-`enc2:`-prefixed value or decrypting a plaintext value is a no-op,
/// detected via [`crate::security::SecretStore::is_encrypted`]. The `field`
/// argument is the dotted config-path (e.g. `mcp.servers`); the impls suffix
/// per-element coordinates (`[<idx>]` for `Vec`, `.<key>` for `HashMap`) so
/// error messages point at the exact failed entry.
pub trait SecretField {
    /// Encrypt every non-empty, not-already-encrypted inner string.
    fn encrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()>;

    /// Inverse of [`Self::encrypt_in_place`].
    fn decrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()>;

    /// Whether the field carries at least one non-empty inner string. Reported
    /// back through [`SecretFieldInfo::is_set`].
    fn is_set(&self) -> bool;
}

impl SecretField for String {
    fn encrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        if !self.is_empty() && !crate::security::SecretStore::is_encrypted(self) {
            *self = store
                .encrypt(self)
                .with_context(|| format!("Failed to encrypt {field}"))?;
        }
        Ok(())
    }

    fn decrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        if crate::security::SecretStore::is_encrypted(self) {
            *self = store
                .decrypt(self)
                .with_context(|| format!("Failed to decrypt {field}"))?;
        }
        Ok(())
    }

    fn is_set(&self) -> bool {
        !self.is_empty()
    }
}

impl SecretField for Option<String> {
    fn encrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        match self {
            Some(inner) => inner.encrypt_in_place(store, field),
            None => Ok(()),
        }
    }

    fn decrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        match self {
            Some(inner) => inner.decrypt_in_place(store, field),
            None => Ok(()),
        }
    }

    fn is_set(&self) -> bool {
        self.as_ref().is_some_and(|v| !v.is_empty())
    }
}

impl SecretField for Vec<String> {
    fn encrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        for (idx, element) in self.iter_mut().enumerate() {
            element.encrypt_in_place(store, &format!("{field}[{idx}]"))?;
        }
        Ok(())
    }

    fn decrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        for (idx, element) in self.iter_mut().enumerate() {
            element.decrypt_in_place(store, &format!("{field}[{idx}]"))?;
        }
        Ok(())
    }

    fn is_set(&self) -> bool {
        !self.is_empty()
    }
}

impl SecretField for std::collections::HashMap<String, String> {
    fn encrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        for (key, value) in self.iter_mut() {
            value.encrypt_in_place(store, &format!("{field}.{key}"))?;
        }
        Ok(())
    }

    fn decrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        for (key, value) in self.iter_mut() {
            value.decrypt_in_place(store, &format!("{field}.{key}"))?;
        }
        Ok(())
    }

    fn is_set(&self) -> bool {
        !self.is_empty()
    }
}

impl SecretField for Option<std::collections::HashMap<String, String>> {
    fn encrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        match self {
            Some(inner) => inner.encrypt_in_place(store, field),
            None => Ok(()),
        }
    }

    fn decrypt_in_place(
        &mut self,
        store: &crate::security::SecretStore,
        field: &str,
    ) -> anyhow::Result<()> {
        match self {
            Some(inner) => inner.decrypt_in_place(store, field),
            None => Ok(()),
        }
    }

    fn is_set(&self) -> bool {
        self.as_ref().is_some_and(|m| !m.is_empty())
    }
}

/// The trait for describing a channel
pub trait ChannelConfig {
    /// human-readable name
    fn name() -> &'static str;
    /// short description
    fn desc() -> &'static str;
}

// Maybe there should be a `&self` as parameter for custom channel/info or what...

pub trait ConfigHandle {
    fn name(&self) -> &'static str;
    fn desc(&self) -> &'static str;
}

/// A menu item for `OnboardUi::select`, with an optional status badge
/// (e.g. `[configured]` / `[not set]`) that backends render next to the label.
#[derive(Debug, Clone)]
pub struct SelectItem {
    pub label: String,
    pub badge: Option<String>,
}

impl SelectItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            badge: None,
        }
    }

    pub fn with_badge(label: impl Into<String>, badge: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            badge: Some(badge.into()),
        }
    }
}

/// Result of a single prompt — either the value the user chose, or a
/// navigation signal. Backends return `Answer::Back` when the user presses
/// the backend's back key (Esc on ratatui / dialoguer). Callers rewind.
#[derive(Debug, Clone)]
pub enum Answer<T> {
    Value(T),
    Back,
}

/// Prompt-surface the onboard orchestrator drives.
///
/// Async is deliberate: the orchestrator is already async (Config::load_or_init,
/// Config::save), and a future gateway-backed onboarder (WebSocket → browser)
/// needs to await network I/O per prompt. A sync trait would force that
/// backend to bridge sync↔async via blocking threads and channels, which
/// starves the tokio runtime under concurrent onboarding sessions. Blocking
/// backends (dialoguer) wrap their calls in `tokio::task::spawn_blocking`.
///
/// Idempotency contract: prompts accept a `current` value and pre-populate it
/// as the default. `secret(has_current=true)` returns `None` when the user
/// declines to rotate; callers then skip the write. The orchestrator never
/// calls `config.set_prop` unless the new value differs from `current`.
#[async_trait::async_trait]
pub trait OnboardUi: Send {
    async fn confirm(&mut self, prompt: &str, default: bool) -> anyhow::Result<Answer<bool>>;

    async fn string(
        &mut self,
        prompt: &str,
        current: Option<&str>,
    ) -> anyhow::Result<Answer<String>>;

    /// `Answer::Value(Some(v))` = new secret entered. `Answer::Value(None)` =
    /// user declined to update an existing secret (only when `has_current`).
    /// `Answer::Back` = rewind.
    async fn secret(
        &mut self,
        prompt: &str,
        has_current: bool,
    ) -> anyhow::Result<Answer<Option<String>>>;

    async fn select(
        &mut self,
        prompt: &str,
        items: &[SelectItem],
        current: Option<usize>,
    ) -> anyhow::Result<Answer<usize>>;

    async fn editor(&mut self, hint: &str, initial: &str) -> anyhow::Result<Answer<String>>;

    /// Announce a new section or subsection. `level == 1` = section
    /// (Providers, Channels, …). `level == 2` = subsection within a section
    /// (Hardware › Transport). Backends render these persistently so every
    /// prompt remains anchored to its phase — rendered like Markdown
    /// headings. `level == 1` resets any prior subsection.
    fn heading(&mut self, level: u8, text: &str);
    fn note(&mut self, msg: &str);
    fn status(&mut self, msg: &str);
    fn warn(&mut self, msg: &str);
}

#[cfg(test)]
mod secret_field_tests {
    use super::SecretField;
    use crate::security::SecretStore;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn store() -> (TempDir, SecretStore) {
        let tmp = TempDir::new().unwrap();
        let store = SecretStore::new(tmp.path(), true);
        (tmp, store)
    }

    #[test]
    fn string_roundtrip_and_idempotent() {
        let (_tmp, store) = store();
        let mut s = String::from("sk-abc");
        s.encrypt_in_place(&store, "test.s").unwrap();
        assert!(SecretStore::is_encrypted(&s));
        let enc1 = s.clone();
        // idempotent: encrypting again must not double-wrap
        s.encrypt_in_place(&store, "test.s").unwrap();
        assert_eq!(s, enc1);
        s.decrypt_in_place(&store, "test.s").unwrap();
        assert_eq!(s, "sk-abc");
    }

    #[test]
    fn string_empty_stays_empty() {
        let (_tmp, store) = store();
        let mut s = String::new();
        s.encrypt_in_place(&store, "test.s").unwrap();
        assert_eq!(s, "");
        assert!(!s.is_set());
    }

    #[test]
    fn option_string_none_is_noop() {
        let (_tmp, store) = store();
        let mut v: Option<String> = None;
        v.encrypt_in_place(&store, "test.o").unwrap();
        v.decrypt_in_place(&store, "test.o").unwrap();
        assert_eq!(v, None);
        assert!(!v.is_set());
    }

    #[test]
    fn option_string_some_roundtrip() {
        let (_tmp, store) = store();
        let mut v: Option<String> = Some("Bearer xyz".into());
        v.encrypt_in_place(&store, "test.o").unwrap();
        assert!(SecretStore::is_encrypted(v.as_ref().unwrap()));
        v.decrypt_in_place(&store, "test.o").unwrap();
        assert_eq!(v.as_deref(), Some("Bearer xyz"));
        assert!(v.is_set());
    }

    #[test]
    fn vec_string_roundtrip_per_element() {
        let (_tmp, store) = store();
        let mut v: Vec<String> = vec!["one".into(), "".into(), "two".into()];
        v.encrypt_in_place(&store, "test.v").unwrap();
        assert!(SecretStore::is_encrypted(&v[0]));
        assert_eq!(v[1], "", "empty element must stay empty");
        assert!(SecretStore::is_encrypted(&v[2]));
        v.decrypt_in_place(&store, "test.v").unwrap();
        assert_eq!(v, vec!["one", "", "two"]);
    }

    #[test]
    fn hashmap_string_string_roundtrip_per_value() {
        let (_tmp, store) = store();
        let mut h: HashMap<String, String> = HashMap::from([
            ("Authorization".into(), "Bearer sk-abc".into()),
            ("X-Trace".into(), "req-123".into()),
        ]);
        h.encrypt_in_place(&store, "mcp.servers.foo.headers")
            .unwrap();
        for v in h.values() {
            assert!(SecretStore::is_encrypted(v));
        }
        h.decrypt_in_place(&store, "mcp.servers.foo.headers")
            .unwrap();
        assert_eq!(
            h.get("Authorization").map(String::as_str),
            Some("Bearer sk-abc")
        );
        assert_eq!(h.get("X-Trace").map(String::as_str), Some("req-123"));
        assert!(h.is_set());
    }

    #[test]
    fn option_hashmap_none_is_noop() {
        let (_tmp, store) = store();
        let mut v: Option<HashMap<String, String>> = None;
        v.encrypt_in_place(&store, "test.oh").unwrap();
        v.decrypt_in_place(&store, "test.oh").unwrap();
        assert!(v.is_none());
        assert!(!v.is_set());
    }

    #[test]
    fn option_hashmap_some_roundtrip() {
        let (_tmp, store) = store();
        let mut v: Option<HashMap<String, String>> =
            Some(HashMap::from([("k".into(), "secret".into())]));
        v.encrypt_in_place(&store, "test.oh").unwrap();
        assert!(SecretStore::is_encrypted(
            v.as_ref().unwrap().get("k").unwrap()
        ));
        v.decrypt_in_place(&store, "test.oh").unwrap();
        assert_eq!(
            v.as_ref().unwrap().get("k").map(String::as_str),
            Some("secret")
        );
        assert!(v.is_set());
    }

    #[test]
    fn hashmap_empty_is_not_set() {
        let h: HashMap<String, String> = HashMap::new();
        assert!(!h.is_set());
        let oh: Option<HashMap<String, String>> = Some(HashMap::new());
        assert!(!oh.is_set());
    }

    #[test]
    fn encrypt_decrypt_failure_message_includes_field_path() {
        let tmp = TempDir::new().unwrap();
        let bad_store = SecretStore::new(tmp.path(), true);
        let mut s = String::from("enc2:not-valid-hex");
        let err = s
            .decrypt_in_place(&bad_store, "mcp.servers.foo.headers.Authorization")
            .expect_err("malformed ciphertext must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("mcp.servers.foo.headers.Authorization"),
            "error must include field path; got: {msg}"
        );
    }
}
