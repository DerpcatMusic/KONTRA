use ni_file::kontakt::StructuredObject;
use std::io::{Cursor, Read};

#[path = "../../sampler-core/tests/support/mod.rs"]
mod heap;
#[path = "support/chunks.rs"]
mod wire;

#[test]
fn structured_children_allocate_each_retained_payload_once_and_keep_outer_bounds() {
    let payload = vec![0x5a; 1 << 20];
    let mut children = wire::chunk(0x34, &payload);
    children.extend(wire::chunk(0x74, &payload));
    let mut bytes = wire::object(0xae, &[1, 2], &[3, 4], &children);
    let end = bytes.len();
    bytes.extend([0xde, 0xad]);
    let mut cursor = Cursor::new(bytes.as_slice());
    let allocated = heap::allocated_bytes(|| {
        let object = StructuredObject::read(&mut cursor).unwrap();
        assert_eq!(object.private_data, [1, 2]);
        assert_eq!(object.public_data, [3, 4]);
        assert_eq!(object.children.len(), 2);
        for (child, id) in object.children.iter().zip([0x34, 0x74]) {
            assert_eq!(child.id, id);
            assert_eq!(child.data, payload);
        }
    });
    println!(
        "children_bytes={} allocated_bytes={allocated}",
        children.len()
    );
    assert_eq!(cursor.position(), end as u64);
    let mut trailing = Vec::new();
    cursor.read_to_end(&mut trailing).unwrap();
    assert_eq!(trailing, [0xde, 0xad]);
    assert!(
        allocated < payload.len() * 3,
        "bounded children should allocate retained payloads, without a second whole-section copy: {allocated}"
    );

    // A child may not borrow bytes from the following enclosing record.
    let child = wire::chunk(0x34, &[1, 2, 3]);
    let mut invalid = wire::object(0xae, &[], &[], &child[..child.len() - 1]);
    invalid.extend([3, 9, 9]);
    assert!(StructuredObject::read(Cursor::new(invalid)).is_err());
    let valid = wire::object(0xae, &[], &[], &child);
    for end in 0..valid.len() {
        assert!(StructuredObject::read(Cursor::new(&valid[..end])).is_err());
    }
}
