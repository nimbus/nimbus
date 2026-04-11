use neovex_core::{Filter, FilterOp};

use super::super::super::read_set::RuntimeIndexRangeRead;

pub(in crate::execution::read_tracking) fn filters_from_runtime_index_read(
    read: &RuntimeIndexRangeRead,
) -> Vec<Filter> {
    let mut filters = Vec::new();
    if let Some(start) = read.start.clone() {
        filters.push(Filter {
            field: read.field.clone(),
            op: if read.start_inclusive {
                FilterOp::Gte
            } else {
                FilterOp::Gt
            },
            value: start,
        });
    }
    if let Some(end) = read.end.clone() {
        filters.push(Filter {
            field: read.field.clone(),
            op: if read.end_inclusive {
                FilterOp::Lte
            } else {
                FilterOp::Lt
            },
            value: end,
        });
    }
    filters
}
