use std::collections::BTreeMap;

/// Code generation options meant to be supported by all languages.
#[derive(Clone, Debug)]
pub struct CodeGeneratorConfig {
    pub(crate) module_name: String,
    pub(crate) external_definitions: ExternalDefinitions,
    pub(crate) comments: DocComments,
    /// When `true`, convert all generated type names from `snake_case` to
    /// `UpperCamelCase` using the `heck` crate.
    pub(crate) use_title_case: bool,
    /// When `true`, emit the root enum types (`resource_root`, `provider_root`,
    /// `datasource_root`) and the top-level `config` struct.  When `false`
    /// (the default for filtered / typed generation), only resource detail
    /// structs and their block-type children are emitted.
    pub(crate) generate_roots: bool,
}

/// Track types definitions provided by external modules.
pub type ExternalDefinitions =
    std::collections::BTreeMap</* module */ String, /* type names */ Vec<String>>;

/// Track documentation to be attached to particular definitions.
pub type DocComments =
    std::collections::BTreeMap</* qualified name */ Vec<String>, /* comment */ String>;

impl CodeGeneratorConfig {
    /// Default config for the given module name.
    pub fn new(module_name: String) -> Self {
        Self {
            module_name,
            external_definitions: BTreeMap::new(),
            comments: BTreeMap::new(),
            use_title_case: false,
            generate_roots: true,
        }
    }

    /// Container names provided by external modules.
    pub fn with_external_definitions(mut self, external_definitions: ExternalDefinitions) -> Self {
        self.external_definitions = external_definitions;
        self
    }

    /// Comments attached to particular entity.
    pub fn with_comments(mut self, mut comments: DocComments) -> Self {
        // Make sure comments end with a (single) newline.
        for comment in comments.values_mut() {
            *comment = format!("{}\n", comment.trim());
        }
        self.comments = comments;
        self
    }

    /// Enable or disable `UpperCamelCase` conversion for generated type names.
    pub fn with_title_case(mut self, enabled: bool) -> Self {
        self.use_title_case = enabled;
        self
    }

    /// Enable or disable generation of root enum / config types.
    pub fn with_generate_roots(mut self, enabled: bool) -> Self {
        self.generate_roots = enabled;
        self
    }
}
