//! Array files through the public `fill`.

use kontra_plan::files::{ArrayKind, fill};
use kontra_vm::Text;

/// A file holding more values than its array is refused whole: nothing is
/// written, not even the values that would fit.
#[test]
fn a_file_larger_than_its_array_leaves_the_array_unchanged() {
    let mut cells = [5i64, 6, 7];
    let mut texts = [Text::EMPTY; 1];
    let filled = fill(
        b"%a\n1\n2\n3\n",
        "%a",
        (0, 2),
        ArrayKind::Int,
        &mut cells,
        &mut texts,
    );
    assert_eq!(filled, None);
    assert_eq!(cells, [5, 6, 7], "the destination is untouched");
}
