use sampler_ir::{Driver, SwitchKeys, SwitchOwner, Switching};

#[test]
fn every_switching_round_trips_through_a_byte() {
    for owner in [SwitchOwner::Native, SwitchOwner::Behavior] {
        for driver in [
            Driver::Keys,
            Driver::Velocity,
            Driver::Channel,
            Driver::Controller,
            Driver::Program,
        ] {
            for keys in [SwitchKeys::Keep, SwitchKeys::Play, SwitchKeys::Swallow] {
                let s = Switching {
                    owner,
                    driver,
                    keys,
                };
                assert_eq!(Switching::from_bits(s.to_bits()), Some(s));
            }
        }
    }
    assert_eq!(Switching::from_bits(0xff), None);
}
