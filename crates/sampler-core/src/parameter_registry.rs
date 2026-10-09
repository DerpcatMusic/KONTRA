//! Plan-local parameter identities and immutable editor metadata. Names never
//! enter rendering; preparation resolves addresses to existing control owners.
use crate::{ControlId, EngineParameterAddress, Error};
use std::collections::BTreeMap;
pub use sampler_ir::ProcessorParameter as ParameterRole;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParameterScope {
    Voice,
    Group(u32),
    Bus(u32),
    Plan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParameterAddress {
    pub scope: ParameterScope,
    pub node: u32,
    pub parameter: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterUnit {
    Linear,
    Normalized,
    Percent,
    Decibels,
    Hertz,
    Seconds,
    Semitones,
    Octaves,
    Frames,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParameterLaw {
    Linear,
    Native(crate::EngineParameterLaw),
}

/// Display hints describe the authored owner rather than a specific UI widget.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParameterDisplay {
    pub group: String,
    pub order: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterDescriptor {
    pub address: ParameterAddress,
    pub control: ControlId,
    pub name: String,
    /// Semantic processor field, independent of display labels.
    pub role: ParameterRole,
    pub unit: ParameterUnit,
    pub range: [f64; 2],
    pub default: f64,
    pub law: ParameterLaw,
    pub display: ParameterDisplay,
}

#[derive(Default)]
pub struct ParameterRegistry {
    descriptors: Vec<ParameterDescriptor>,
    addresses: BTreeMap<ParameterAddress, usize>,
    aliases: BTreeMap<EngineParameterAddress, ParameterAddress>,
}

impl ParameterRegistry {
    pub fn register(&mut self, descriptor: ParameterDescriptor) -> Result<(), Error> {
        let [low, high] = descriptor.range;
        if !low.is_finite()
            || !high.is_finite()
            || low > high
            || !(high - low).is_finite()
            || descriptor.address.parameter >= u32::MAX - 13
            || matches!(descriptor.law, ParameterLaw::Native(law) if !law.valid())
            || !(low..=high).contains(&descriptor.default)
            || self.addresses.contains_key(&descriptor.address)
        {
            return Err(Error::InvalidInput);
        }
        self.addresses
            .insert(descriptor.address, self.descriptors.len());
        self.descriptors.push(descriptor);
        Ok(())
    }

    pub fn alias_native(
        &mut self,
        native: EngineParameterAddress,
        address: ParameterAddress,
    ) -> Result<(), Error> {
        if !self.addresses.contains_key(&address)
            || self.aliases.get(&native).is_some_and(|old| *old != address)
        {
            return Err(Error::InvalidInput);
        }
        self.aliases.insert(native, address);
        Ok(())
    }

    pub fn prepare(self) -> Result<PreparedParameterRegistry, Error> {
        Ok(PreparedParameterRegistry {
            descriptors: self.descriptors.into_boxed_slice(),
            addresses: self.addresses.iter().map(|(a, i)| (*a, *i)).collect(),
            aliases: self
                .aliases
                .into_iter()
                .map(|(native, address)| (native, self.addresses[&address]))
                .collect(),
        })
    }
}

#[derive(Default)]
pub struct PreparedParameterRegistry {
    descriptors: Box<[ParameterDescriptor]>,
    addresses: Box<[(ParameterAddress, usize)]>,
    aliases: Box<[(EngineParameterAddress, usize)]>,
}

impl PreparedParameterRegistry {
    pub fn resolve(&self, address: ParameterAddress) -> Option<usize> {
        self.addresses
            .binary_search_by_key(&address, |(address, _)| *address)
            .ok()
            .map(|i| self.addresses[i].1)
    }
    pub fn resolve_native(&self, address: EngineParameterAddress) -> Option<usize> {
        self.aliases
            .binary_search_by_key(&address, |(address, _)| *address)
            .ok()
            .map(|i| self.aliases[i].1)
    }
    pub fn descriptor(&self, lane: usize) -> Option<&ParameterDescriptor> {
        self.descriptors.get(lane)
    }
    pub fn descriptors(&self) -> impl ExactSizeIterator<Item = &ParameterDescriptor> {
        self.descriptors.iter()
    }
}

// Legacy constructors retain their exact reductions while callers adopt addresses.
#[allow(non_upper_case_globals, non_snake_case)]
impl ParameterAddress {
    const fn legacy(node: u32, parameter: u32) -> Self {
        Self {
            scope: ParameterScope::Voice,
            node,
            parameter: u32::MAX - parameter,
        }
    }
    pub const Attenuate: Self = Self::legacy(0, 0);
    pub const Decibels: Self = Self::legacy(0, 1);
    pub const Pan: Self = Self::legacy(0, 2);
    pub const Pitch: Self = Self::legacy(0, 3);
    pub const Cutoff: Self = Self::legacy(0, 4);
    pub const Resonance: Self = Self::legacy(0, 5);
    pub const Tone: Self = Self::legacy(0, 11);
    pub const SampleStart: Self = Self::legacy(0, 12);
    pub const fn ProcessorCutoff(node: u32) -> Self {
        Self::legacy(node, 6)
    }
    pub const fn ProcessorResonance(node: u32) -> Self {
        Self::legacy(node, 7)
    }
    pub const fn ProcessorNativeCutoff(node: u32) -> Self {
        Self::legacy(node, 8)
    }
    pub const fn ProcessorNativeResonance(node: u32) -> Self {
        Self::legacy(node, 9)
    }
    pub const fn ProcessorNativeGain(node: u32) -> Self {
        Self::legacy(node, 10)
    }
}

impl crate::Prepared {
    pub fn parameter_registry(&self) -> &PreparedParameterRegistry {
        &self.parameter_registry
    }
    pub fn with_parameter_registry(mut self, registry: ParameterRegistry) -> Result<Self, Error> {
        let registry = registry.prepare()?;
        for descriptor in registry.descriptors() {
            self.control_index(descriptor.control)?;
        }
        for (native, lane) in &registry.aliases {
            if !self
                .engine_parameters
                .iter()
                .any(|b| b.address == *native && b.control == registry.descriptors[*lane].control)
            {
                return Err(Error::InvalidInput);
            }
        }
        self.parameter_registry = registry;
        Ok(self)
    }
}

impl crate::Runtime {
    /// Immutable schema for an off-audio reader of the addressed generation.
    pub fn parameter_registry(
        &self,
        plan: crate::PlanId,
    ) -> Result<&PreparedParameterRegistry, Error> {
        Ok(&self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .parameter_registry)
    }
}
