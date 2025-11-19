#[derive(Debug, Clone, PartialEq)]
pub enum ImportStep {
    FolderIdentification,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportPhase {
    FolderSelection,
    MetadataDetection,
    ExactLookup,
    ManualSearch,
    Confirmation,
}
