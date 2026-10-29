//! Trace selected translator lookup decisions without changing the laws.

pub(crate) fn eq_band_index(param: &str, group: usize, route: usize, target: usize) -> Option<u8> {
    let result = crate::laws::eq_band_index(param);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.eq_band_index.v1"),
        location: format!("groups/{group}/routes/{route}/targets/{target}/eq_band"),
        input: format!("{param:?}"),
        output: format!("{result:?}"),
    });
    result
}
