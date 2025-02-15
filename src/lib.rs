#![warn(missing_docs)]

//! This crate provides an [Egui](https://github.com/emilk/egui) integration for the [Bevy](https://github.com/bevyengine/bevy) game engine.
//!
//! **Trying out:**
//!
//! An example WASM project is live at [vladbat00.github.io/bevy_egui_web_showcase](https://vladbat00.github.io/bevy_egui_web_showcase/index.html) [[source](https://github.com/vladbat00/bevy_egui_web_showcase)].
//!
//! **Features:**
//! - Desktop and web platforms support
//! - Clipboard
//! - Opening URLs
//! - Multiple windows support (see [./examples/two_windows.rs](https://github.com/vladbat00/bevy_egui/blob/v0.29.0/examples/two_windows.rs))
//! - Paint callback support (see [./examples/paint_callback.rs](https://github.com/vladbat00/bevy_egui/blob/v0.29.0/examples/paint_callback.rs))
//! - Mobile web virtual keyboard (still rough support and only works without prevent_default_event_handling set to false on the WindowPlugin primary_window)
//!
//! ## Dependencies
//!
//! On Linux, this crate requires certain parts of [XCB](https://xcb.freedesktop.org/) to be installed on your system. On Debian-based systems, these can be installed with the following command:
//!
//! ```bash
//! sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
//! ```
//!
//! ## Usage
//!
//! Here's a minimal usage example:
//!
//! ```no_run,rust
//! use bevy::prelude::*;
//! use bevy_egui::{egui, EguiContexts, EguiPlugin};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(EguiPlugin)
//!         // Systems that create Egui widgets should be run during the `CoreSet::Update` set,
//!         // or after the `EguiSet::BeginPass` system (which belongs to the `CoreSet::PreUpdate` set).
//!         .add_systems(Update, ui_example_system)
//!         .run();
//! }
//!
//! fn ui_example_system(mut contexts: EguiContexts) {
//!     egui::Window::new("Hello").show(contexts.ctx_mut(), |ui| {
//!         ui.label("world");
//!     });
//! }
//! ```
//!
//! For a more advanced example, see [examples/ui.rs](https://github.com/vladbat00/bevy_egui/blob/v0.20.1/examples/ui.rs).
//!
//! ```bash
//! cargo run --example ui
//! ```
//!
//! ## See also
//!
//! - [`bevy-inspector-egui`](https://github.com/jakobhellermann/bevy-inspector-egui)

#[cfg(all(
    feature = "manage_clipboard",
    target_arch = "wasm32",
    not(web_sys_unstable_apis)
))]
compile_error!(include_str!("../static/error_web_sys_unstable_apis.txt"));

/// Egui render node.
#[cfg(feature = "render")]
pub mod egui_node;
/// Egui render node for rendering to a texture.
/// Plugin systems for the render app.
#[cfg(feature = "render")]
pub mod render_systems;
/// Plugin systems.
pub mod systems;
/// Mobile web keyboard hacky input support
#[cfg(target_arch = "wasm32")]
mod text_agent;
/// Clipboard management for web
#[cfg(all(
    feature = "manage_clipboard",
    target_arch = "wasm32",
    web_sys_unstable_apis
))]
pub mod web_clipboard;

pub use egui;

use crate::systems::*;
#[cfg(target_arch = "wasm32")]
use crate::text_agent::{
    install_text_agent, is_mobile_safari, process_safari_virtual_keyboard, propagate_text,
    SafariVirtualKeyboardHack, TextAgentChannel, VirtualTouchInfo,
};
#[cfg(feature = "render")]
use crate::{
    egui_node::{EguiPipeline, EGUI_SHADER_HANDLE},
    render_systems::{EguiTransforms, ExtractedEguiManagedTextures},
};
#[cfg(all(
    feature = "manage_clipboard",
    not(any(target_arch = "wasm32", target_os = "android"))
))]
use arboard::Clipboard;
use bevy_app::prelude::*;
#[cfg(feature = "render")]
use bevy_asset::{load_internal_asset, AssetEvent, Assets, Handle};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{
    prelude::*,
    query::{QueryData, QueryEntityError},
    schedule::apply_deferred,
    system::SystemParam,
};
#[cfg(feature = "render")]
use bevy_image::{Image, ImageSampler};
use bevy_input::InputSystem;
#[cfg(feature = "render")]
use bevy_picking::{
    backend::{HitData, PointerHits},
    pointer::{PointerId, PointerLocation},
};
use bevy_reflect::Reflect;
#[cfg(feature = "render")]
use bevy_render::{
    camera::NormalizedRenderTarget,
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::{LoadOp, SpecializedRenderPipelines},
    ExtractSchedule, Render, RenderApp, RenderSet,
};
use bevy_window::{PrimaryWindow, SystemCursorIcon, Window};
use bevy_winit::cursor::CursorIcon;
#[cfg(all(
    feature = "manage_clipboard",
    not(any(target_arch = "wasm32", target_os = "android"))
))]
use std::cell::{RefCell, RefMut};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Adds all Egui resources and render graph nodes.
pub struct EguiPlugin;

/// A component for storing Egui context settings.
#[derive(Clone, Debug, Component, Reflect)]
#[cfg_attr(feature = "render", derive(ExtractComponent))]
pub struct EguiSettings {
    /// Controls if Egui is run manually.
    ///
    /// If set to `true`, a user is expected to call [`egui::Context::run`] or [`egui::Context::begin_pass`] and [`egui::Context::end_pass`] manually.
    pub run_manually: bool,
    /// Global scale factor for Egui widgets (`1.0` by default).
    ///
    /// This setting can be used to force the UI to render in physical pixels regardless of DPI as follows:
    /// ```rust
    /// use bevy::{prelude::*, window::PrimaryWindow};
    /// use bevy_egui::EguiSettings;
    ///
    /// fn update_ui_scale_factor(mut windows: Query<(&mut EguiSettings, &Window), With<PrimaryWindow>>) {
    ///     if let Ok((mut egui_settings, window)) = windows.get_single_mut() {
    ///         egui_settings.scale_factor = 1.0 / window.scale_factor();
    ///     }
    /// }
    /// ```
    pub scale_factor: f32,
    /// Is used as a default value for hyperlink [target](https://www.w3schools.com/tags/att_a_target.asp) hints.
    /// If not specified, `_self` will be used. Only matters in a web browser.
    #[cfg(feature = "open_url")]
    pub default_open_url_target: Option<String>,
    /// Controls if Egui should capture pointer input when using [`bevy_picking`].
    #[cfg(feature = "render")]
    pub capture_pointer_input: bool,
}

// Just to keep the PartialEq
impl PartialEq for EguiSettings {
    #[allow(clippy::let_and_return)]
    fn eq(&self, other: &Self) -> bool {
        let eq = self.scale_factor == other.scale_factor;
        #[cfg(feature = "open_url")]
        let eq = eq && self.default_open_url_target == other.default_open_url_target;
        eq
    }
}

impl Default for EguiSettings {
    fn default() -> Self {
        Self {
            run_manually: false,
            scale_factor: 1.0,
            #[cfg(feature = "open_url")]
            default_open_url_target: None,
            #[cfg(feature = "render")]
            capture_pointer_input: true,
        }
    }
}

/// Is used for storing Egui context input.
///
/// It gets reset during the [`EguiSet::ProcessInput`] system.
#[derive(Component, Clone, Debug, Default, Deref, DerefMut)]
pub struct EguiInput(pub egui::RawInput);

/// Is used to store Egui context output.
#[derive(Component, Clone, Default, Deref, DerefMut)]
pub struct EguiFullOutput(pub Option<egui::FullOutput>);

/// A resource for accessing clipboard.
///
/// The resource is available only if `manage_clipboard` feature is enabled.
#[cfg(all(feature = "manage_clipboard", not(target_os = "android")))]
#[derive(Default, bevy_ecs::system::Resource)]
pub struct EguiClipboard {
    #[cfg(not(target_arch = "wasm32"))]
    clipboard: thread_local::ThreadLocal<Option<RefCell<Clipboard>>>,
    #[cfg(all(target_arch = "wasm32", web_sys_unstable_apis))]
    clipboard: web_clipboard::WebClipboard,
}

#[cfg(all(
    feature = "manage_clipboard",
    not(target_os = "android"),
    not(all(target_arch = "wasm32", not(web_sys_unstable_apis)))
))]
impl EguiClipboard {
    /// Sets clipboard contents.
    pub fn set_contents(&mut self, contents: &str) {
        self.set_contents_impl(contents);
    }

    /// Sets the internal buffer of clipboard contents.
    /// This buffer is used to remember the contents of the last "Paste" event.
    #[cfg(all(target_arch = "wasm32", web_sys_unstable_apis))]
    pub fn set_contents_internal(&mut self, contents: &str) {
        self.clipboard.set_contents_internal(contents);
    }

    /// Gets clipboard contents. Returns [`None`] if clipboard provider is unavailable or returns an error.
    #[must_use]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_contents(&mut self) -> Option<String> {
        self.get_contents_impl()
    }

    /// Gets clipboard contents. Returns [`None`] if clipboard provider is unavailable or returns an error.
    #[must_use]
    #[cfg(all(target_arch = "wasm32", web_sys_unstable_apis))]
    pub fn get_contents(&mut self) -> Option<String> {
        self.get_contents_impl()
    }

    /// Receives a clipboard event sent by the `copy`/`cut`/`paste` listeners.
    #[cfg(all(target_arch = "wasm32", web_sys_unstable_apis))]
    pub fn try_receive_clipboard_event(&self) -> Option<web_clipboard::WebClipboardEvent> {
        self.clipboard.try_receive_clipboard_event()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn set_contents_impl(&mut self, contents: &str) {
        if let Some(mut clipboard) = self.get() {
            if let Err(err) = clipboard.set_text(contents.to_owned()) {
                bevy_log::error!("Failed to set clipboard contents: {:?}", err);
            }
        }
    }

    #[cfg(all(target_arch = "wasm32", web_sys_unstable_apis))]
    fn set_contents_impl(&mut self, contents: &str) {
        self.clipboard.set_contents(contents);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn get_contents_impl(&mut self) -> Option<String> {
        if let Some(mut clipboard) = self.get() {
            match clipboard.get_text() {
                Ok(contents) => return Some(contents),
                Err(err) => bevy_log::error!("Failed to get clipboard contents: {:?}", err),
            }
        };
        None
    }

    #[cfg(all(target_arch = "wasm32", web_sys_unstable_apis))]
    #[allow(clippy::unnecessary_wraps)]
    fn get_contents_impl(&mut self) -> Option<String> {
        self.clipboard.get_contents()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn get(&self) -> Option<RefMut<Clipboard>> {
        self.clipboard
            .get_or(|| {
                Clipboard::new()
                    .map(RefCell::new)
                    .map_err(|err| {
                        bevy_log::error!("Failed to initialize clipboard: {:?}", err);
                    })
                    .ok()
            })
            .as_ref()
            .map(|cell| cell.borrow_mut())
    }
}

/// Is used for storing Egui shapes and textures delta.
#[derive(Component, Clone, Default, Debug)]
#[cfg_attr(feature = "render", derive(ExtractComponent))]
pub struct EguiRenderOutput {
    /// Pairs of rectangles and paint commands.
    ///
    /// The field gets populated during the [`EguiSet::ProcessOutput`] system (belonging to bevy's [`PostUpdate`]) and reset during `EguiNode::update`.
    pub paint_jobs: Vec<egui::ClippedPrimitive>,

    /// The change in egui textures since last frame.
    pub textures_delta: egui::TexturesDelta,
}

impl EguiRenderOutput {
    /// Returns `true` if the output has no Egui shapes and no textures delta
    pub fn is_empty(&self) -> bool {
        self.paint_jobs.is_empty() && self.textures_delta.is_empty()
    }
}

/// Is used for storing Egui output.
#[derive(Component, Clone, Default)]
pub struct EguiOutput {
    /// The field gets updated during the [`EguiSet::ProcessOutput`] system (belonging to [`PostUpdate`]).
    pub platform_output: egui::PlatformOutput,
}

/// A component for storing `bevy_egui` context.
#[derive(Clone, Component, Default)]
#[cfg_attr(feature = "render", derive(ExtractComponent))]
pub struct EguiContext {
    ctx: egui::Context,
    mouse_position: egui::Pos2,
    pointer_touch_id: Option<u64>,
    has_sent_ime_enabled: bool,
}

impl EguiContext {
    /// Borrows the underlying Egui context immutably.
    ///
    /// Even though the mutable borrow isn't necessary, as the context is wrapped into `RwLock`,
    /// using the immutable getter is gated with the `immutable_ctx` feature. Using the immutable
    /// borrow is discouraged as it may cause unpredictable blocking in UI systems.
    ///
    /// When the context is queried with `&mut EguiContext`, the Bevy scheduler is able to make
    /// sure that the context isn't accessed concurrently and can perform other useful work
    /// instead of busy-waiting.
    #[cfg(feature = "immutable_ctx")]
    #[must_use]
    pub fn get(&self) -> &egui::Context {
        &self.ctx
    }

    /// Borrows the underlying Egui context mutably.
    ///
    /// Even though the mutable borrow isn't necessary, as the context is wrapped into `RwLock`,
    /// using the immutable getter is gated with the `immutable_ctx` feature. Using the immutable
    /// borrow is discouraged as it may cause unpredictable blocking in UI systems.
    ///
    /// When the context is queried with `&mut EguiContext`, the Bevy scheduler is able to make
    /// sure that the context isn't accessed concurrently and can perform other useful work
    /// instead of busy-waiting.
    #[must_use]
    pub fn get_mut(&mut self) -> &mut egui::Context {
        &mut self.ctx
    }
}

#[cfg(not(feature = "render"))]
type EguiContextsFilter = With<Window>;

#[cfg(feature = "render")]
type EguiContextsFilter = Or<(With<Window>, With<EguiRenderToImage>)>;

#[derive(SystemParam)]
/// A helper SystemParam that provides a way to get [`EguiContext`] with less boilerplate and
/// combines a proxy interface to the [`EguiUserTextures`] resource.
pub struct EguiContexts<'w, 's> {
    q: Query<
        'w,
        's,
        (
            Entity,
            &'static mut EguiContext,
            Option<&'static PrimaryWindow>,
        ),
        EguiContextsFilter,
    >,
    #[cfg(feature = "render")]
    user_textures: ResMut<'w, EguiUserTextures>,
}

impl EguiContexts<'_, '_> {
    /// Egui context of the primary window.
    #[must_use]
    pub fn ctx_mut(&mut self) -> &mut egui::Context {
        self.try_ctx_mut()
            .expect("`EguiContexts::ctx_mut` was called for an uninitialized context (primary window), make sure your system is run after [`EguiSet::InitContexts`] (or [`EguiStartupSet::InitContexts`] for startup systems)")
    }

    /// Fallible variant of [`EguiContexts::ctx_mut`].
    #[must_use]
    pub fn try_ctx_mut(&mut self) -> Option<&mut egui::Context> {
        self.q
            .iter_mut()
            .find_map(|(_window_entity, ctx, primary_window)| {
                if primary_window.is_some() {
                    Some(ctx.into_inner().get_mut())
                } else {
                    None
                }
            })
    }

    /// Egui context of a specific entity.
    #[must_use]
    pub fn ctx_for_entity_mut(&mut self, entity: Entity) -> &mut egui::Context {
        self.try_ctx_for_entity_mut(entity)
            .unwrap_or_else(|| panic!("`EguiContexts::ctx_for_window_mut` was called for an uninitialized context (entity {entity:?}), make sure your system is run after [`EguiSet::InitContexts`] (or [`EguiStartupSet::InitContexts`] for startup systems)"))
    }

    /// Fallible variant of [`EguiContexts::ctx_for_entity_mut`].
    #[must_use]
    #[track_caller]
    pub fn try_ctx_for_entity_mut(&mut self, entity: Entity) -> Option<&mut egui::Context> {
        self.q
            .iter_mut()
            .find_map(|(window_entity, ctx, _primary_window)| {
                if window_entity == entity {
                    Some(ctx.into_inner().get_mut())
                } else {
                    None
                }
            })
    }

    /// Allows to get multiple contexts at the same time. This function is useful when you want
    /// to get multiple window contexts without using the `immutable_ctx` feature.
    #[track_caller]
    pub fn ctx_for_entities_mut<const N: usize>(
        &mut self,
        ids: [Entity; N],
    ) -> Result<[&mut egui::Context; N], QueryEntityError> {
        self.q
            .get_many_mut(ids)
            .map(|arr| arr.map(|(_window_entity, ctx, _primary_window)| ctx.into_inner().get_mut()))
    }

    /// Egui context of the primary window.
    ///
    /// Even though the mutable borrow isn't necessary, as the context is wrapped into `RwLock`,
    /// using the immutable getter is gated with the `immutable_ctx` feature. Using the immutable
    /// borrow is discouraged as it may cause unpredictable blocking in UI systems.
    ///
    /// When the context is queried with `&mut EguiContext`, the Bevy scheduler is able to make
    /// sure that the context isn't accessed concurrently and can perform other useful work
    /// instead of busy-waiting.
    #[cfg(feature = "immutable_ctx")]
    #[must_use]
    pub fn ctx(&self) -> &egui::Context {
        self.try_ctx()
            .expect("`EguiContexts::ctx` was called for an uninitialized context (primary window), make sure your system is run after [`EguiSet::InitContexts`] (or [`EguiStartupSet::InitContexts`] for startup systems)")
    }

    /// Fallible variant of [`EguiContexts::ctx`].
    ///
    /// Even though the mutable borrow isn't necessary, as the context is wrapped into `RwLock`,
    /// using the immutable getter is gated with the `immutable_ctx` feature. Using the immutable
    /// borrow is discouraged as it may cause unpredictable blocking in UI systems.
    ///
    /// When the context is queried with `&mut EguiContext`, the Bevy scheduler is able to make
    /// sure that the context isn't accessed concurrently and can perform other useful work
    /// instead of busy-waiting.
    #[cfg(feature = "immutable_ctx")]
    #[must_use]
    pub fn try_ctx(&self) -> Option<&egui::Context> {
        self.q
            .iter()
            .find_map(|(_window_entity, ctx, primary_window)| {
                if primary_window.is_some() {
                    Some(ctx.get())
                } else {
                    None
                }
            })
    }

    /// Egui context of a specific window.
    ///
    /// Even though the mutable borrow isn't necessary, as the context is wrapped into `RwLock`,
    /// using the immutable getter is gated with the `immutable_ctx` feature. Using the immutable
    /// borrow is discouraged as it may cause unpredictable blocking in UI systems.
    ///
    /// When the context is queried with `&mut EguiContext`, the Bevy scheduler is able to make
    /// sure that the context isn't accessed concurrently and can perform other useful work
    /// instead of busy-waiting.
    #[must_use]
    #[cfg(feature = "immutable_ctx")]
    pub fn ctx_for_entity(&self, entity: Entity) -> &egui::Context {
        self.try_ctx_for_entity(entity)
            .unwrap_or_else(|| panic!("`EguiContexts::ctx_for_entity` was called for an uninitialized context (entity {entity:?}), make sure your system is run after [`EguiSet::InitContexts`] (or [`EguiStartupSet::InitContexts`] for startup systems)"))
    }

    /// Fallible variant of [`EguiContexts::ctx_for_entity`].
    ///
    /// Even though the mutable borrow isn't necessary, as the context is wrapped into `RwLock`,
    /// using the immutable getter is gated with the `immutable_ctx` feature. Using the immutable
    /// borrow is discouraged as it may cause unpredictable blocking in UI systems.
    ///
    /// When the context is queried with `&mut EguiContext`, the Bevy scheduler is able to make
    /// sure that the context isn't accessed concurrently and can perform other useful work
    /// instead of busy-waiting.
    #[must_use]
    #[track_caller]
    #[cfg(feature = "immutable_ctx")]
    pub fn try_ctx_for_entity(&self, entity: Entity) -> Option<&egui::Context> {
        self.q
            .iter()
            .find_map(|(window_entity, ctx, _primary_window)| {
                if window_entity == entity {
                    Some(ctx.get())
                } else {
                    None
                }
            })
    }

    /// Can accept either a strong or a weak handle.
    ///
    /// You may want to pass a weak handle if you control removing texture assets in your
    /// application manually and you don't want to bother with cleaning up textures in Egui.
    ///
    /// You'll want to pass a strong handle if a texture is used only in Egui and there are no
    /// handle copies stored anywhere else.
    #[cfg(feature = "render")]
    pub fn add_image(&mut self, image: Handle<Image>) -> egui::TextureId {
        self.user_textures.add_image(image)
    }

    /// Removes the image handle and an Egui texture id associated with it.
    #[cfg(feature = "render")]
    #[track_caller]
    pub fn remove_image(&mut self, image: &Handle<Image>) -> Option<egui::TextureId> {
        self.user_textures.remove_image(image)
    }

    /// Returns an associated Egui texture id.
    #[cfg(feature = "render")]
    #[must_use]
    #[track_caller]
    pub fn image_id(&self, image: &Handle<Image>) -> Option<egui::TextureId> {
        self.user_textures.image_id(image)
    }
}

/// Contexts with this component will render UI to a specified image.
///
/// You can create an entity just with this component, `bevy_egui` will initialize an [`EguiContext`]
/// automatically.
#[cfg(feature = "render")]
#[derive(Component, Clone, Debug, ExtractComponent)]
pub struct EguiRenderToImage {
    /// A handle of an image to render to.
    pub handle: Handle<Image>,
    /// Customizable [`LoadOp`] for the render node which will be created for this context.
    ///
    /// You'll likely want [`LoadOp::Clear`], unless you need to draw the UI on top of existing
    /// pixels of the image.
    pub load_op: LoadOp<wgpu_types::Color>,
}

#[cfg(feature = "render")]
impl EguiRenderToImage {
    /// Creates a component from an image handle and sets [`EguiRenderToImage::load_op`] to [`LoadOp::Clear].
    pub fn new(handle: Handle<Image>) -> Self {
        Self {
            handle,
            load_op: LoadOp::Clear(wgpu_types::Color::TRANSPARENT),
        }
    }
}

/// A resource for storing `bevy_egui` user textures.
#[derive(Clone, bevy_ecs::system::Resource, Default, ExtractResource)]
#[cfg(feature = "render")]
pub struct EguiUserTextures {
    textures: bevy_utils::HashMap<Handle<Image>, u64>,
    last_texture_id: u64,
}

#[cfg(feature = "render")]
impl EguiUserTextures {
    /// Can accept either a strong or a weak handle.
    ///
    /// You may want to pass a weak handle if you control removing texture assets in your
    /// application manually and you don't want to bother with cleaning up textures in Egui.
    ///
    /// You'll want to pass a strong handle if a texture is used only in Egui and there are no
    /// handle copies stored anywhere else.
    pub fn add_image(&mut self, image: Handle<Image>) -> egui::TextureId {
        let id = *self.textures.entry(image.clone()).or_insert_with(|| {
            let id = self.last_texture_id;
            bevy_log::debug!("Add a new image (id: {}, handle: {:?})", id, image);
            self.last_texture_id += 1;
            id
        });
        egui::TextureId::User(id)
    }

    /// Removes the image handle and an Egui texture id associated with it.
    pub fn remove_image(&mut self, image: &Handle<Image>) -> Option<egui::TextureId> {
        let id = self.textures.remove(image);
        bevy_log::debug!("Remove image (id: {:?}, handle: {:?})", id, image);
        id.map(egui::TextureId::User)
    }

    /// Returns an associated Egui texture id.
    #[must_use]
    pub fn image_id(&self, image: &Handle<Image>) -> Option<egui::TextureId> {
        self.textures
            .get(image)
            .map(|&id| egui::TextureId::User(id))
    }
}

/// Stores physical size and scale factor, is used as a helper to calculate logical size.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "render", derive(ExtractComponent))]
pub struct RenderTargetSize {
    /// Physical width
    pub physical_width: f32,
    /// Physical height
    pub physical_height: f32,
    /// Scale factor
    pub scale_factor: f32,
}

impl RenderTargetSize {
    fn new(physical_width: f32, physical_height: f32, scale_factor: f32) -> Self {
        Self {
            physical_width,
            physical_height,
            scale_factor,
        }
    }

    /// Returns the width of the render target.
    #[inline]
    pub fn width(&self) -> f32 {
        self.physical_width / self.scale_factor
    }

    /// Returns the height of the render target.
    #[inline]
    pub fn height(&self) -> f32 {
        self.physical_height / self.scale_factor
    }
}

/// The names of `bevy_egui` nodes.
pub mod node {
    /// The main egui pass.
    pub const EGUI_PASS: &str = "egui_pass";
}

#[derive(SystemSet, Clone, Hash, Debug, Eq, PartialEq)]
/// The `bevy_egui` plugin startup system sets.
pub enum EguiStartupSet {
    /// Initializes Egui contexts for available windows.
    InitContexts,
}

/// The `bevy_egui` plugin system sets.
#[derive(SystemSet, Clone, Hash, Debug, Eq, PartialEq)]
pub enum EguiSet {
    /// Initializes Egui contexts for newly created render targets.
    InitContexts,
    /// Reads Egui inputs (keyboard, mouse, etc) and writes them into the [`EguiInput`] resource.
    ///
    /// To modify the input, you can hook your system like this:
    ///
    /// `system.after(EguiSet::ProcessInput).before(EguiSet::BeginPass)`.
    ProcessInput,
    /// Begins the `egui` pass.
    BeginPass,
    /// Processes the [`EguiOutput`] resource.
    ProcessOutput,
}

impl Plugin for EguiPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<EguiSettings>();

        #[cfg(feature = "render")]
        {
            app.init_resource::<EguiManagedTextures>();
            app.init_resource::<EguiUserTextures>();
            app.add_plugins(ExtractResourcePlugin::<EguiUserTextures>::default());
            app.add_plugins(ExtractResourcePlugin::<ExtractedEguiManagedTextures>::default());
            app.add_plugins(ExtractComponentPlugin::<EguiContext>::default());
            app.add_plugins(ExtractComponentPlugin::<EguiSettings>::default());
            app.add_plugins(ExtractComponentPlugin::<RenderTargetSize>::default());
            app.add_plugins(ExtractComponentPlugin::<EguiRenderOutput>::default());
            app.add_plugins(ExtractComponentPlugin::<EguiRenderToImage>::default());
        }

        #[cfg(target_arch = "wasm32")]
        app.init_non_send_resource::<SubscribedEvents>();

        #[cfg(all(feature = "manage_clipboard", not(target_os = "android")))]
        app.init_resource::<EguiClipboard>();

        #[cfg(all(
            feature = "manage_clipboard",
            target_arch = "wasm32",
            web_sys_unstable_apis
        ))]
        {
            app.add_systems(PreStartup, web_clipboard::startup_setup_web_events);
        }

        app.add_systems(
            PreStartup,
            (
                setup_new_windows_system,
                #[cfg(feature = "render")]
                setup_render_to_image_handles_system,
                apply_deferred,
                update_contexts_system,
            )
                .chain()
                .in_set(EguiStartupSet::InitContexts),
        );

        app.add_systems(
            PreUpdate,
            (
                setup_new_windows_system,
                #[cfg(feature = "render")]
                setup_render_to_image_handles_system,
                apply_deferred,
                update_contexts_system,
            )
                .chain()
                .in_set(EguiSet::InitContexts),
        );
        app.add_systems(
            PreUpdate,
            process_input_system
                .in_set(EguiSet::ProcessInput)
                .after(InputSystem)
                .after(EguiSet::InitContexts),
        );
        #[cfg(target_arch = "wasm32")]
        {
            use std::sync::{LazyLock, Mutex};

            let maybe_window_plugin = app.get_added_plugins::<bevy_window::WindowPlugin>();

            if !maybe_window_plugin.is_empty()
                && maybe_window_plugin[0].primary_window.is_some()
                && maybe_window_plugin[0]
                    .primary_window
                    .as_ref()
                    .unwrap()
                    .prevent_default_event_handling
            {
                app.init_resource::<TextAgentChannel>();

                let (sender, receiver) = crossbeam_channel::unbounded();
                static TOUCH_INFO: LazyLock<Mutex<VirtualTouchInfo>> =
                    LazyLock::new(|| Mutex::new(VirtualTouchInfo::default()));

                app.insert_resource(SafariVirtualKeyboardHack {
                    sender,
                    receiver,
                    touch_info: &TOUCH_INFO,
                });

                app.add_systems(
                    PreStartup,
                    install_text_agent
                        .in_set(EguiSet::ProcessInput)
                        .after(process_input_system)
                        .after(InputSystem)
                        .after(EguiSet::InitContexts),
                );

                app.add_systems(
                    PreUpdate,
                    propagate_text
                        .in_set(EguiSet::ProcessInput)
                        .after(process_input_system)
                        .after(InputSystem)
                        .after(EguiSet::InitContexts),
                );

                if is_mobile_safari() {
                    app.add_systems(
                        PostUpdate,
                        process_safari_virtual_keyboard.after(process_output_system),
                    );
                }
            }
        }
        app.add_systems(
            PreUpdate,
            begin_pass_system
                .in_set(EguiSet::BeginPass)
                .after(EguiSet::ProcessInput),
        );

        app.add_systems(PostUpdate, end_pass_system.before(EguiSet::ProcessOutput));
        app.add_systems(
            PostUpdate,
            process_output_system.in_set(EguiSet::ProcessOutput),
        );
        #[cfg(feature = "render")]
        app.add_systems(PostUpdate, capture_pointer_input);

        #[cfg(feature = "render")]
        app.add_systems(
            PostUpdate,
            update_egui_textures_system.after(EguiSet::ProcessOutput),
        )
        .add_systems(
            Render,
            render_systems::prepare_egui_transforms_system.in_set(RenderSet::Prepare),
        )
        .add_systems(
            Render,
            render_systems::queue_bind_groups_system.in_set(RenderSet::Queue),
        )
        .add_systems(
            Render,
            render_systems::queue_pipelines_system.in_set(RenderSet::Queue),
        )
        .add_systems(Last, free_egui_textures_system);

        #[cfg(feature = "render")]
        load_internal_asset!(
            app,
            EGUI_SHADER_HANDLE,
            "egui.wgsl",
            bevy_render::render_resource::Shader::from_wgsl
        );
    }

    #[cfg(feature = "render")]
    fn finish(&self, app: &mut App) {
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<egui_node::EguiPipeline>()
                .init_resource::<SpecializedRenderPipelines<EguiPipeline>>()
                .init_resource::<EguiTransforms>()
                .add_systems(
                    // Seems to be just the set to add/remove nodes, as it'll run before
                    // `RenderSet::ExtractCommands` where render nodes get updated.
                    ExtractSchedule,
                    (
                        render_systems::setup_new_window_nodes_system,
                        render_systems::teardown_window_nodes_system,
                        render_systems::setup_new_render_to_image_nodes_system,
                        render_systems::teardown_render_to_image_nodes_system,
                    ),
                )
                .add_systems(
                    Render,
                    render_systems::prepare_egui_transforms_system.in_set(RenderSet::Prepare),
                )
                .add_systems(
                    Render,
                    render_systems::queue_bind_groups_system.in_set(RenderSet::Queue),
                )
                .add_systems(
                    Render,
                    render_systems::queue_pipelines_system.in_set(RenderSet::Queue),
                );
        }
    }
}

/// Queries all the Egui related components.
#[derive(QueryData)]
#[query_data(mutable)]
#[non_exhaustive]
pub struct EguiContextQuery {
    /// Window entity.
    pub render_target: Entity,
    /// Egui context associated with the render target.
    pub ctx: &'static mut EguiContext,
    /// Settings associated with the context.
    pub egui_settings: &'static mut EguiSettings,
    /// Encapsulates [`egui::RawInput`].
    pub egui_input: &'static mut EguiInput,
    /// Encapsulates [`egui::FullOutput`].
    pub egui_full_output: &'static mut EguiFullOutput,
    /// Egui shapes and textures delta.
    pub render_output: &'static mut EguiRenderOutput,
    /// Encapsulates [`egui::PlatformOutput`].
    pub egui_output: &'static mut EguiOutput,
    /// Stores physical size of the window and its scale factor.
    pub render_target_size: &'static mut RenderTargetSize,
    /// [`Window`] component, when rendering to a window.
    pub window: Option<&'static mut Window>,
    /// [`CursorIcon`] component.
    pub cursor: Option<&'static mut CursorIcon>,
    /// [`EguiRenderToImage`] component, when rendering to a texture.
    #[cfg(feature = "render")]
    pub render_to_image: Option<&'static mut EguiRenderToImage>,
}

impl EguiContextQueryItem<'_> {
    fn ime_event_enable(&mut self) {
        if !self.ctx.has_sent_ime_enabled {
            self.egui_input
                .events
                .push(egui::Event::Ime(egui::ImeEvent::Enabled));
            self.ctx.has_sent_ime_enabled = true;
        }
    }

    fn ime_event_disable(&mut self) {
        if self.ctx.has_sent_ime_enabled {
            self.egui_input
                .events
                .push(egui::Event::Ime(egui::ImeEvent::Disabled));
            self.ctx.has_sent_ime_enabled = false;
        }
    }
}

/// Contains textures allocated and painted by Egui.
#[cfg(feature = "render")]
#[derive(bevy_ecs::system::Resource, Deref, DerefMut, Default)]
pub struct EguiManagedTextures(pub bevy_utils::HashMap<(Entity, u64), EguiManagedTexture>);

/// Represents a texture allocated and painted by Egui.
#[cfg(feature = "render")]
pub struct EguiManagedTexture {
    /// Assets store handle.
    pub handle: Handle<Image>,
    /// Stored in full so we can do partial updates (which bevy doesn't support).
    pub color_image: egui::ColorImage,
}

/// Adds bevy_egui components to newly created windows.
pub fn setup_new_windows_system(
    mut commands: Commands,
    new_windows: Query<Entity, (Added<Window>, Without<EguiContext>)>,
) {
    for window in new_windows.iter() {
        commands.entity(window).insert((
            EguiContext::default(),
            EguiSettings::default(),
            EguiRenderOutput::default(),
            EguiInput::default(),
            EguiFullOutput::default(),
            EguiOutput::default(),
            RenderTargetSize::default(),
            CursorIcon::System(SystemCursorIcon::Default),
        ));
    }
}

/// The ordering value used for bevy_picking.
#[cfg(feature = "render")]
pub const PICKING_ORDER: f32 = 1_000_000.0;
/// Captures pointers on egui windows for bevy_picking.
#[cfg(feature = "render")]
pub fn capture_pointer_input(
    pointers: Query<(&PointerId, &PointerLocation)>,
    mut egui_context: Query<(Entity, &mut EguiContext, &EguiSettings)>,
    mut output: EventWriter<PointerHits>,
) {
    for (pointer, location) in pointers
        .iter()
        .filter_map(|(i, p)| p.location.as_ref().map(|l| (i, l)))
    {
        if let NormalizedRenderTarget::Window(id) = location.target {
            if let Ok((entity, mut ctx, settings)) = egui_context.get_mut(id.entity()) {
                if settings.capture_pointer_input && ctx.get_mut().wants_pointer_input() {
                    let entry = (entity, HitData::new(entity, 0.0, None, None));
                    output.send(PointerHits::new(
                        *pointer,
                        Vec::from([entry]),
                        PICKING_ORDER,
                    ));
                }
            }
        }
    }
}

/// Adds bevy_egui components to newly created windows.
#[cfg(feature = "render")]
pub fn setup_render_to_image_handles_system(
    mut commands: Commands,
    new_render_to_image_targets: Query<Entity, (Added<EguiRenderToImage>, Without<EguiContext>)>,
) {
    for render_to_image_target in new_render_to_image_targets.iter() {
        commands.entity(render_to_image_target).insert((
            EguiContext::default(),
            EguiSettings::default(),
            EguiRenderOutput::default(),
            EguiInput::default(),
            EguiFullOutput::default(),
            EguiOutput::default(),
            RenderTargetSize::default(),
        ));
    }
}

/// Updates textures painted by Egui.
#[cfg(feature = "render")]
#[allow(clippy::type_complexity)]
pub fn update_egui_textures_system(
    mut egui_render_output: Query<
        (Entity, &mut EguiRenderOutput),
        Or<(With<Window>, With<EguiRenderToImage>)>,
    >,
    mut egui_managed_textures: ResMut<EguiManagedTextures>,
    mut image_assets: ResMut<Assets<Image>>,
) {
    for (entity, mut egui_render_output) in egui_render_output.iter_mut() {
        let set_textures = std::mem::take(&mut egui_render_output.textures_delta.set);

        for (texture_id, image_delta) in set_textures {
            let color_image = egui_node::as_color_image(image_delta.image);

            let texture_id = match texture_id {
                egui::TextureId::Managed(texture_id) => texture_id,
                egui::TextureId::User(_) => continue,
            };

            let sampler = ImageSampler::Descriptor(
                egui_node::texture_options_as_sampler_descriptor(&image_delta.options),
            );
            if let Some(pos) = image_delta.pos {
                // Partial update.
                if let Some(managed_texture) = egui_managed_textures.get_mut(&(entity, texture_id))
                {
                    // TODO: when bevy supports it, only update the part of the texture that changes.
                    update_image_rect(&mut managed_texture.color_image, pos, &color_image);
                    let image =
                        egui_node::color_image_as_bevy_image(&managed_texture.color_image, sampler);
                    managed_texture.handle = image_assets.add(image);
                } else {
                    bevy_log::warn!("Partial update of a missing texture (id: {:?})", texture_id);
                }
            } else {
                // Full update.
                let image = egui_node::color_image_as_bevy_image(&color_image, sampler);
                let handle = image_assets.add(image);
                egui_managed_textures.insert(
                    (entity, texture_id),
                    EguiManagedTexture {
                        handle,
                        color_image,
                    },
                );
            }
        }
    }

    fn update_image_rect(dest: &mut egui::ColorImage, [x, y]: [usize; 2], src: &egui::ColorImage) {
        for sy in 0..src.height() {
            for sx in 0..src.width() {
                dest[(x + sx, y + sy)] = src[(sx, sy)];
            }
        }
    }
}

#[cfg(feature = "render")]
#[allow(clippy::type_complexity)]
fn free_egui_textures_system(
    mut egui_user_textures: ResMut<EguiUserTextures>,
    mut egui_render_output: Query<
        (Entity, &mut EguiRenderOutput),
        Or<(With<Window>, With<EguiRenderToImage>)>,
    >,
    mut egui_managed_textures: ResMut<EguiManagedTextures>,
    mut image_assets: ResMut<Assets<Image>>,
    mut image_events: EventReader<AssetEvent<Image>>,
) {
    for (entity, mut egui_render_output) in egui_render_output.iter_mut() {
        let free_textures = std::mem::take(&mut egui_render_output.textures_delta.free);
        for texture_id in free_textures {
            if let egui::TextureId::Managed(texture_id) = texture_id {
                let managed_texture = egui_managed_textures.remove(&(entity, texture_id));
                if let Some(managed_texture) = managed_texture {
                    image_assets.remove(&managed_texture.handle);
                }
            }
        }
    }

    for image_event in image_events.read() {
        if let AssetEvent::Removed { id } = image_event {
            egui_user_textures.remove_image(&Handle::<Image>::Weak(*id));
        }
    }
}

/// Helper function for outputting a String from a JsValue
#[cfg(target_arch = "wasm32")]
pub fn string_from_js_value(value: &JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:#?}"))
}

#[cfg(target_arch = "wasm32")]
struct EventClosure<T> {
    target: web_sys::EventTarget,
    event_name: String,
    closure: wasm_bindgen::closure::Closure<dyn FnMut(T)>,
}

/// Stores event listeners.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct SubscribedEvents {
    #[cfg(all(feature = "manage_clipboard", web_sys_unstable_apis))]
    clipboard_event_closures: Vec<EventClosure<web_sys::ClipboardEvent>>,
    composition_event_closures: Vec<EventClosure<web_sys::CompositionEvent>>,
    keyboard_event_closures: Vec<EventClosure<web_sys::KeyboardEvent>>,
    input_event_closures: Vec<EventClosure<web_sys::InputEvent>>,
    touch_event_closures: Vec<EventClosure<web_sys::TouchEvent>>,
}

#[cfg(target_arch = "wasm32")]
impl SubscribedEvents {
    /// Use this method to unsubscribe from all stored events, this can be useful
    /// for gracefully destroying a Bevy instance in a page.
    pub fn unsubscribe_from_all_events(&mut self) {
        #[cfg(all(feature = "manage_clipboard", web_sys_unstable_apis))]
        Self::unsubscribe_from_events(&mut self.clipboard_event_closures);
        Self::unsubscribe_from_events(&mut self.composition_event_closures);
        Self::unsubscribe_from_events(&mut self.keyboard_event_closures);
        Self::unsubscribe_from_events(&mut self.input_event_closures);
        Self::unsubscribe_from_events(&mut self.touch_event_closures);
    }

    fn unsubscribe_from_events<T>(events: &mut Vec<EventClosure<T>>) {
        let events_to_unsubscribe = std::mem::take(events);

        if !events_to_unsubscribe.is_empty() {
            for event in events_to_unsubscribe {
                if let Err(err) = event.target.remove_event_listener_with_callback(
                    event.event_name.as_str(),
                    event.closure.as_ref().unchecked_ref(),
                ) {
                    log::error!(
                        "Failed to unsubscribe from event: {}",
                        string_from_js_value(&err)
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::PluginGroup,
        render::{settings::WgpuSettings, RenderPlugin},
        winit::WinitPlugin,
        DefaultPlugins,
    };

    #[test]
    fn test_readme_deps() {
        version_sync::assert_markdown_deps_updated!("README.md");
    }

    #[test]
    fn test_headless_mode() {
        App::new()
            .add_plugins(
                DefaultPlugins
                    .set(RenderPlugin {
                        render_creation: bevy::render::settings::RenderCreation::Automatic(
                            WgpuSettings {
                                backends: None,
                                ..Default::default()
                            },
                        ),
                        ..Default::default()
                    })
                    .build()
                    .disable::<WinitPlugin>(),
            )
            .add_plugins(EguiPlugin)
            .update();
    }
}
