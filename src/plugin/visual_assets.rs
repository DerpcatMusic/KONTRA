//! Editor/worker ownership of demanded artwork. Audio never owns these payloads.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VisualDemand {
    pub pictures: Vec<String>,
    pub metadata: Vec<String>,
    pub native_images: Vec<String>,
    pub native_fonts: Vec<String>,
    pub wallpaper: bool,
}
impl VisualDemand {
    fn canonicalize(&mut self) {
        for names in [&mut self.pictures, &mut self.metadata, &mut self.native_images, &mut self.native_fonts] {
            names.sort(); names.dedup();
        }
    }
}

pub(super) struct Request {
    slot: usize,
    path: PathBuf,
    program: u32,
    generation: u64,
    epoch: u64,
    revision: u64,
    demand: VisualDemand,
    instrument: Arc<Instrument>,
    interface: Option<Arc<Interface>>,
    missing_metadata: Vec<String>,
}

impl Request {
    fn capture(slot: usize, v: &PartView, generation: u64) -> Option<Self> {
        let instrument = v.instrument.clone()?;
        Some(Self { slot, path: instrument.path.clone(), program: v.program, generation,
            epoch: v.script_epoch, revision: v.visual_revision, demand: v.visual_demand.clone(),
            missing_metadata: v.visual_demand.metadata.iter().filter(|name| !v.picture_metadata.contains_key(name.as_str())).cloned().collect(),
            instrument, interface: v.interface.clone() })
    }
    fn matches(&self, v: &PartView, generation: u64) -> bool {
        v.visual_revision == self.revision && v.visual_demand == self.demand
            && v.script_epoch == self.epoch && v.program == self.program
            && v.instrument.as_ref().is_some_and(|i| i.path == self.path)
            && generation == self.generation
    }
}

impl SamplerParams {
    /// Called by the editor for each visible part, and with empty demand for hidden parts.
    pub(crate) fn request_visual_assets(self: &Arc<Self>, slot: usize, mut demand: VisualDemand) {
        demand.canonicalize();
        let Some(shared) = self.shared.part(slot) else { return; };
        let request = {
            let mut view = self.shared.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(v) = view.parts.get_mut(slot) else { return; };
            if v.instrument.is_none() || v.visual_demand == demand { return; }
            v.visual_demand = demand.clone();
            v.visual_revision = v.visual_revision.wrapping_add(1);
            // Replacing the maps releases real image Arcs on the editor, not audio.
            v.pictures = Arc::new(v.pictures.iter().filter(|(name, _)| demand.pictures.contains(name))
                .map(|(name, picture)| (name.clone(), picture.clone())).collect());
            if !demand.wallpaper { v.wallpaper = None; }
            if let Some(package) = &v.native_ui {
                v.native_ui = Some(Arc::new(package.retain_visuals(&demand.native_images, &demand.native_fonts)));
            }
            // One worker drains the latest demand; animation cannot spawn a decode backlog.
            if v.visual_worker || demand == VisualDemand::default() { return; }
            v.visual_worker = true;
            Request::capture(slot, v, shared.generation.load(Ordering::Acquire)).unwrap()
        };
        let p = self.clone();
        if let Err(error) = std::thread::Builder::new().name("kontakto-visual-assets".into()).spawn(move || run(&p, request)) {
            let mut view = self.shared.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(v) = view.parts.get_mut(slot) {
                v.visual_worker = false;
                v.visual_demand = VisualDemand::default();
                v.wallpaper_status = format!("Artwork worker unavailable: {error}");
            }
        }
    }
}

fn run(p: &SamplerParams, mut request: Request) {
    loop {
        let retained = {
            let view = p.shared.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(v) = view.parts.get(request.slot) else { return; };
            let generation = p.shared.part(request.slot).map_or(0, |part| part.generation.load(Ordering::Acquire));
            // Avoid starting expensive work for a request already superseded.
            request.matches(v, generation).then(|| (v.pictures.clone(), v.wallpaper.clone(), v.native_ui.clone()))
        };
        if let Some((pictures, wallpaper, native)) = retained {
            let assets = load_retained(&request, (*pictures).clone(), wallpaper, native);
            publish(p, &request, assets);
        }
        let mut view = p.shared.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(v) = view.parts.get_mut(request.slot) else { return; };
        let generation = p.shared.part(request.slot).map_or(0, |part| part.generation.load(Ordering::Acquire));
        if request.matches(v, generation) || v.instrument.is_none() || v.visual_demand == VisualDemand::default() {
            v.visual_worker = false;
            return;
        }
        request = Request::capture(request.slot, v, generation).unwrap();
    }
}

pub(super) struct Assets {
    pictures: HashMap<String, Arc<artwork::Picture>>,
    metadata: HashMap<String, artwork::PictureMetadata>,
    wallpaper: Option<Arc<artwork::Picture>>,
    native: Option<Arc<crate::native_ui::Package>>,
    errors: Vec<String>,
}

#[cfg(test)]
pub(super) fn load(request: &Request) -> Assets {
    load_retained(request, HashMap::new(), None, None)
}

fn load_retained(request: &Request, mut pictures: HashMap<String, Arc<artwork::Picture>>,
    wallpaper: Option<Arc<artwork::Picture>>, native: Option<Arc<crate::native_ui::Package>>) -> Assets {
    let missing: Vec<_> = request.demand.pictures.iter().filter(|name| !pictures.contains_key(name.as_str())).collect();
    let (more, mut errors) = if missing.is_empty() { (HashMap::new(), Vec::new()) } else {
        artwork::pictures_report(&request.path, missing.into_iter().map(String::as_str))
    };
    let mut metadata: HashMap<_, _> = more.iter().filter_map(|(name, picture)| artwork::PictureMetadata::of(picture).map(|meta| (name.clone(), meta))).collect();
    pictures.extend(more);
    let missing: Vec<_> = request.missing_metadata.iter().filter(|name| !metadata.contains_key(name.as_str())).collect();
    if !missing.is_empty() {
        let (more, issues) = artwork::picture_metadata_report(&request.path, missing.into_iter().map(String::as_str));
        metadata.extend(more); errors.extend(issues);
    }
    let wallpaper = if request.demand.wallpaper && wallpaper.is_none() {
        match artwork::performance(&request.instrument, request.interface.as_deref()) {
            Ok(value) => value, Err(error) => { errors.push(error); None }
        }
    } else { wallpaper };
    let mut native = native.map(|package| (*package).clone());
    let images: Vec<_> = request.demand.native_images.iter().filter(|name| !native.as_ref().is_some_and(|p| p.images.contains_key(&name.to_lowercase()))).cloned().collect();
    let fonts: Vec<_> = request.demand.native_fonts.iter().filter(|name| !native.as_ref().is_some_and(|p| p.fonts.contains_key(&name.to_lowercase()))).cloned().collect();
    if !images.is_empty() || !fonts.is_empty() {
        match crate::native_ui::Package::load_visuals(&request.path, &images, &fonts) {
            Ok(package) => native.get_or_insert_with(Default::default).merge_visuals(package),
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    Assets {pictures, metadata, wallpaper, native: native.map(Arc::new), errors}
}

pub(super) fn publish(p: &SamplerParams, request: &Request, assets: Assets) -> bool {
    let Some(shared) = p.shared.part(request.slot) else { return false; };
    let mut view = p.shared.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(v) = view.parts.get_mut(request.slot) else { return false; };
    if !request.matches(v, shared.generation.load(Ordering::Acquire)) { return false; }
    v.pictures = Arc::new(assets.pictures);
    let mut metadata = (*v.picture_metadata).clone(); metadata.extend(assets.metadata);
    v.picture_metadata = Arc::new(metadata);
    if request.demand.wallpaper { v.wallpaper = assets.wallpaper; }
    if let Some(package) = assets.native { v.native_ui = Some(package); }
    if !assets.errors.is_empty() { v.wallpaper_status = assets.errors.join("; "); }
    true
}

impl PartView {
    /// Unique decoded pixels currently owned by this part's published artwork.
    pub(crate) fn visual_bytes(&self) -> (usize, usize) {
        let mut images = HashMap::<usize, usize>::new();
        for image in self.pictures.values().chain(self.wallpaper.iter()).flat_map(|p| &p.frames) {
            images.insert(Arc::as_ptr(image) as usize, image.rgba.len());
        }
        let mut raw = 0;
        if let Some(package) = &self.native_ui {
            for image in package.images.values() { images.insert(Arc::as_ptr(image) as usize, image.rgba.len()); }
            raw = package.members.values().map(|bytes| bytes.len()).sum();
        }
        (images.values().sum(), raw)
    }
}

#[cfg(test)]
#[path = "visual_assets_tests.rs"]
mod tests;
