use std::any::{Any, TypeId};
use std::collections::HashMap;

#[derive(Debug)]
pub enum TransferError {
    MissingConverter { from: TypeId, to: TypeId },
    DowncastFailed { expected: TypeId },
    ConversionFailed(String),
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingConverter { from, to } => {
                write!(
                    f,
                    "no payload converter registered from {:?} to {:?}",
                    from, to
                )
            }
            Self::DowncastFailed { expected } => {
                write!(f, "failed to downcast payload to {:?}", expected)
            }
            Self::ConversionFailed(reason) => write!(f, "payload conversion failed: {reason}"),
        }
    }
}

impl std::error::Error for TransferError {}

pub trait PaneState: Any + Send {
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

impl<T> PaneState for T
where
    T: Any + Send,
{
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

type ConverterFn = Box<
    dyn Fn(Box<dyn PaneState>) -> Result<Box<dyn PaneState>, TransferError> + Send + Sync + 'static,
>;

#[derive(Default)]
pub struct PayloadRegistry {
    converters: HashMap<(TypeId, TypeId), ConverterFn>,
}

impl PayloadRegistry {
    pub fn register<From, To>(
        &mut self,
        converter: impl Fn(From) -> To + Send + Sync + 'static,
    ) -> &mut Self
    where
        From: PaneState + 'static,
        To: PaneState + 'static,
    {
        self.converters.insert(
            (TypeId::of::<From>(), TypeId::of::<To>()),
            Box::new(move |payload| {
                let any = payload.into_any();
                let source = any
                    .downcast::<From>()
                    .map_err(|_| TransferError::DowncastFailed {
                        expected: TypeId::of::<From>(),
                    })?;
                let converted = converter(*source);
                Ok(Box::new(converted))
            }),
        );
        self
    }

    pub fn convert(
        &self,
        payload: Box<dyn PaneState>,
        target_type: TypeId,
    ) -> Result<Box<dyn PaneState>, TransferError> {
        let source_type = payload.as_ref().type_id();
        if source_type == target_type {
            return Ok(payload);
        }
        let converter = self.converters.get(&(source_type, target_type)).ok_or(
            TransferError::MissingConverter {
                from: source_type,
                to: target_type,
            },
        )?;
        converter(payload)
    }

    pub fn can_convert(&self, from: TypeId, to: TypeId) -> bool {
        from == to || self.converters.contains_key(&(from, to))
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use super::{PaneState, PayloadRegistry, TransferError};

    #[derive(Debug)]
    struct BufferState {
        value: String,
    }

    #[derive(Debug)]
    struct ShellState {
        cmd: String,
    }

    #[test]
    fn registry_converts_registered_payload_types() {
        let mut registry = PayloadRegistry::default();
        registry.register(|from: BufferState| ShellState {
            cmd: format!("nvim {}", from.value),
        });

        let result = registry
            .convert(
                Box::new(BufferState {
                    value: "main.rs".into(),
                }),
                TypeId::of::<ShellState>(),
            )
            .expect("converter should be found");

        let any = PaneState::into_any(result);
        let shell = any
            .downcast::<ShellState>()
            .expect("converted payload should downcast");
        assert_eq!(shell.cmd, "nvim main.rs");
    }

    #[test]
    fn registry_returns_structured_error_for_missing_converter() {
        let registry = PayloadRegistry::default();
        let err = match registry.convert(
            Box::new(BufferState {
                value: "main.rs".into(),
            }),
            TypeId::of::<ShellState>(),
        ) {
            Ok(_) => panic!("missing converter should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, TransferError::MissingConverter { .. }));
    }
}
