use crate::domain::states::{JobItemStatus, JobStatus};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransitionError {
    #[error("Transición inválida de Job: de '{from}' a '{to}'")]
    InvalidJobTransition { from: JobStatus, to: JobStatus },
    #[error("Transición inválida de JobItem: de '{from}' a '{to}'")]
    InvalidJobItemTransition {
        from: JobItemStatus,
        to: JobItemStatus,
    },
}

pub struct TransitionValidator;

impl TransitionValidator {
    /// Valida una transición de estado de Job.
    /// Retorna `Ok(true)` si la transición es válida y produce un cambio de estado.
    /// Retorna `Ok(false)` si el estado destino es idéntico al actual (idempotencia inocua).
    /// Retorna `Err(TransitionError)` si la transición está prohibida.
    pub fn validate_job_transition(
        from: JobStatus,
        to: JobStatus,
    ) -> Result<bool, TransitionError> {
        if from == to {
            return Ok(false);
        }

        let is_valid = match from {
            JobStatus::Created => matches!(
                to,
                JobStatus::Extracting
                    | JobStatus::Queued
                    | JobStatus::Cancelled
                    | JobStatus::Failed
            ),
            JobStatus::Extracting => matches!(
                to,
                JobStatus::Queued
                    | JobStatus::Running
                    | JobStatus::Paused
                    | JobStatus::Cancelling
                    | JobStatus::Cancelled
                    | JobStatus::Failed
            ),
            JobStatus::Queued => matches!(
                to,
                JobStatus::Running
                    | JobStatus::Paused
                    | JobStatus::Cancelling
                    | JobStatus::Cancelled
            ),
            JobStatus::Running => matches!(
                to,
                JobStatus::Paused
                    | JobStatus::Cancelling
                    | JobStatus::Completed
                    | JobStatus::CompletedWithErrors
                    | JobStatus::Failed
            ),
            JobStatus::Paused => matches!(
                to,
                JobStatus::Queued
                    | JobStatus::Running
                    | JobStatus::Cancelling
                    | JobStatus::Cancelled
            ),
            JobStatus::Cancelling => matches!(to, JobStatus::Cancelled | JobStatus::Failed),
            // Estados terminales no admiten transición a ningún otro estado
            JobStatus::Cancelled
            | JobStatus::Completed
            | JobStatus::CompletedWithErrors
            | JobStatus::Failed => false,
        };

        if is_valid {
            Ok(true)
        } else {
            Err(TransitionError::InvalidJobTransition { from, to })
        }
    }

    /// Valida una transición de estado de JobItem.
    /// Retorna `Ok(true)` si la transición es válida y produce un cambio.
    /// Retorna `Ok(false)` si el estado destino es idéntico al actual (idempotente).
    /// Retorna `Err(TransitionError)` si la transición está prohibida.
    pub fn validate_job_item_transition(
        from: JobItemStatus,
        to: JobItemStatus,
    ) -> Result<bool, TransitionError> {
        if from == to {
            return Ok(false);
        }

        let is_valid = match from {
            JobItemStatus::Pending => matches!(
                to,
                JobItemStatus::Queued
                    | JobItemStatus::WaitingForDuplicate
                    | JobItemStatus::Skipped
                    | JobItemStatus::Cancelled
            ),
            JobItemStatus::WaitingForDuplicate => matches!(
                to,
                JobItemStatus::Queued
                    | JobItemStatus::Pending
                    | JobItemStatus::Skipped
                    | JobItemStatus::Cancelled
                    | JobItemStatus::Failed
            ),
            JobItemStatus::Queued => matches!(
                to,
                JobItemStatus::Downloading
                    | JobItemStatus::WaitingForDuplicate
                    | JobItemStatus::Paused
                    | JobItemStatus::Cancelled
            ),
            JobItemStatus::Downloading => matches!(
                to,
                JobItemStatus::Validating
                    | JobItemStatus::Converting
                    | JobItemStatus::Interrupted
                    | JobItemStatus::Paused
                    | JobItemStatus::RetryWait
                    | JobItemStatus::Failed
                    | JobItemStatus::Cancelled
            ),
            JobItemStatus::Validating => matches!(
                to,
                JobItemStatus::Converting
                    | JobItemStatus::Tagging
                    | JobItemStatus::Completed
                    | JobItemStatus::Interrupted
                    | JobItemStatus::Paused
                    | JobItemStatus::RetryWait
                    | JobItemStatus::Failed
                    | JobItemStatus::Cancelled
            ),
            JobItemStatus::Converting => matches!(
                to,
                JobItemStatus::Tagging
                    | JobItemStatus::Validating
                    | JobItemStatus::Interrupted
                    | JobItemStatus::Paused
                    | JobItemStatus::RetryWait
                    | JobItemStatus::Failed
                    | JobItemStatus::Cancelled
            ),
            JobItemStatus::Tagging => matches!(
                to,
                JobItemStatus::Validating
                    | JobItemStatus::Completed
                    | JobItemStatus::Interrupted
                    | JobItemStatus::Paused
                    | JobItemStatus::RetryWait
                    | JobItemStatus::Failed
                    | JobItemStatus::Cancelled
            ),
            JobItemStatus::Interrupted => matches!(
                to,
                JobItemStatus::Queued
                    | JobItemStatus::Paused
                    | JobItemStatus::Cancelled
                    | JobItemStatus::RetryWait
                    | JobItemStatus::Failed
            ),
            JobItemStatus::Paused => matches!(
                to,
                JobItemStatus::Queued | JobItemStatus::Cancelled | JobItemStatus::Interrupted
            ),
            JobItemStatus::RetryWait => matches!(
                to,
                JobItemStatus::Queued | JobItemStatus::Cancelled | JobItemStatus::Failed
            ),
            // Estados terminales no admiten transición a otros estados
            JobItemStatus::Completed
            | JobItemStatus::Skipped
            | JobItemStatus::Cancelled
            | JobItemStatus::Failed => false,
        };

        if is_valid {
            Ok(true)
        } else {
            Err(TransitionError::InvalidJobItemTransition { from, to })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_transitions_valid_and_idempotent() {
        assert_eq!(
            TransitionValidator::validate_job_transition(JobStatus::Created, JobStatus::Extracting),
            Ok(true)
        );
        assert_eq!(
            TransitionValidator::validate_job_transition(
                JobStatus::Extracting,
                JobStatus::Extracting
            ),
            Ok(false)
        );
        assert_eq!(
            TransitionValidator::validate_job_transition(JobStatus::Running, JobStatus::Completed),
            Ok(true)
        );
        assert!(TransitionValidator::validate_job_transition(
            JobStatus::Completed,
            JobStatus::Running
        )
        .is_err());
    }

    #[test]
    fn test_job_item_transitions_valid_and_idempotent() {
        assert_eq!(
            TransitionValidator::validate_job_item_transition(
                JobItemStatus::Downloading,
                JobItemStatus::Interrupted
            ),
            Ok(true)
        );
        assert_eq!(
            TransitionValidator::validate_job_item_transition(
                JobItemStatus::Interrupted,
                JobItemStatus::Interrupted
            ),
            Ok(false)
        );
        assert_eq!(
            TransitionValidator::validate_job_item_transition(
                JobItemStatus::Interrupted,
                JobItemStatus::Queued
            ),
            Ok(true)
        );
        assert!(TransitionValidator::validate_job_item_transition(
            JobItemStatus::Completed,
            JobItemStatus::Downloading
        )
        .is_err());
    }
}
