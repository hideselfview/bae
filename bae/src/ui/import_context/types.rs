#[derive(Debug, Clone, PartialEq)]
pub enum ImportStep {
    FolderIdentification,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportPhase {
    FolderSelection,
    ReleaseSelection,
    MetadataDetection,
    ExactLookup,
    ManualSearch,
    Confirmation,
}
