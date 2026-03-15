use std::any::Any;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeExecutionMode {
    SourceFocused,
    TargetFocused,
}

pub struct MergePreparation {
    payload: Option<Box<dyn Any + Send>>,
}

impl MergePreparation {
    pub fn none() -> Self {
        Self { payload: None }
    }

    pub fn with_payload<T>(payload: T) -> Self
    where
        T: Send + 'static,
    {
        Self {
            payload: Some(Box::new(payload)),
        }
    }

    pub fn into_payload<T>(self) -> Option<T>
    where
        T: Send + 'static,
    {
        self.payload
            .and_then(|payload| payload.downcast::<T>().ok())
            .map(|typed| *typed)
    }

    pub fn map_payload<T>(self, update: impl FnOnce(T) -> T) -> Self
    where
        T: Send + 'static,
    {
        let Some(payload) = self.payload else {
            return self;
        };
        match payload.downcast::<T>() {
            Ok(typed) => Self::with_payload(update(*typed)),
            Err(payload) => Self {
                payload: Some(payload),
            },
        }
    }
}

impl Default for MergePreparation {
    fn default() -> Self {
        Self::none()
    }
}

#[derive(Debug, Clone)]
pub struct SourcePaneMerge<Meta = ()> {
    pub pane_id: u64,
    pub meta: Meta,
}

impl<Meta> SourcePaneMerge<Meta> {
    pub fn new(pane_id: u64, meta: Meta) -> Self {
        Self { pane_id, meta }
    }
}
