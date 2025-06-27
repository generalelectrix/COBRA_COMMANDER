use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Attribute, Data, DeriveInput, Expr, Field, Fields, Ident, Lit, Meta};

/// Register a fixture with the global patch registry.
#[proc_macro]
pub fn register_patcher(input: TokenStream) -> TokenStream {
    let ident = parse_macro_input!(input as Ident);
    register_patcher_impl(&ident).into()
}

fn register_patcher_impl(ident: &Ident) -> proc_macro2::TokenStream {
    quote! {
        use linkme::distributed_slice;
        use crate::fixture::patch::PATCHERS;

        #[distributed_slice(PATCHERS)]
        static PATCHER: crate::fixture::patch::Patcher = <#ident>::patch;
    }
}

/// Derive the PatchAnimatedFixture trait on a fixture struct.
/// The fixture must implement Default.
/// Use the channel_count attribute to specify the DMX channel count.
/// Registers the fixture type with the patch.
#[proc_macro_derive(PatchAnimatedFixture, attributes(channel_count))]
pub fn derive_patch_animated_fixture(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, attrs, .. } = parse_macro_input!(input as DeriveInput);

    let channel_count = get_attr_and_usize_payload(&attrs, "channel_count")
        .expect("channel_count attribute is missing");

    let name = ident.to_string();

    let register = register_patcher_impl(&ident);

    quote! {
        impl crate::fixture::patch::PatchAnimatedFixture for #ident {
            const NAME: FixtureType = FixtureType(#name);
            fn channel_count(&self, _render_mode: Option<crate::fixture::RenderMode>) -> usize {
                #channel_count
            }
            fn new(_options: &mut crate::config::Options) -> anyhow::Result<(Self, Option<RenderMode>)> {
                Ok((Self::default(), None))
            }
        }

        #register
    }
    .into()
}

/// Derive the PatchFixture trait on a fixture struct.
/// The fixture must implement Default.
/// Use the channel_count attribute to specify the DMX channel count.
/// Registers the fixture type with the patch.
#[proc_macro_derive(PatchFixture, attributes(channel_count))]
pub fn derive_patch_fixture(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, attrs, .. } = parse_macro_input!(input as DeriveInput);

    let channel_count = get_attr_and_usize_payload(&attrs, "channel_count")
        .expect("channel_count attribute is missing");

    let name = ident.to_string();

    let register = register_patcher_impl(&ident);

    quote! {
        impl crate::fixture::patch::PatchFixture for #ident {
            const NAME: FixtureType = FixtureType(#name);
            fn channel_count(&self, _render_mode: Option<crate::fixture::RenderMode>) -> usize {
                #channel_count
            }
            fn new(_options: &mut crate::config::Options) -> anyhow::Result<(Self, Option<RenderMode>)> {
                Ok((Self::default(), None))
            }
        }

        #register
    }
    .into()
}

/// Derive the EmitState trait on a fixture struct.
///
/// Fields that do not have an emit_state method can be skipped with #[skip_emit].
/// Fields that implement OscControl as well as EmitState can be forced to emit
/// with the OscControl method with the #[force_osc_control] attribute.
/// Fields that may or may not be present depending on configuration can be
/// defined as an `Option<T>` and marked with the #[optional] attribute, which
/// will handle the optionality.
#[proc_macro_derive(EmitState, attributes(skip_emit, force_osc_control, optional))]
pub fn derive_emit_state(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(input as DeriveInput);

    let Data::Struct(struct_data) = data else {
        panic!("Can only derive EmitState for structs.");
    };
    let Fields::Named(fields) = struct_data.fields else {
        panic!("Can only derive EmitState for named structs.");
    };
    let mut lines = quote! {};
    for field in fields.named.iter() {
        if field_has_attr(field, "skip_emit") {
            continue;
        }
        let Some(ident) = &field.ident else {
            continue;
        };

        // Assume we have bound the field to a local with the same identifier.
        let emit_state_call = if field_has_attr(field, "force_osc_control") {
            quote! {
                crate::fixture::control::OscControl::emit_state(#ident, emitter);
            }
        } else {
            quote! {
                #ident.emit_state(emitter);
            }
        };

        lines = insert_optional_call(
            field_has_attr(field, "optional"),
            false,
            ident,
            emit_state_call,
            lines,
        );
    }
    quote! {
        impl crate::fixture::EmitState for #ident {
            fn emit_state(&self, emitter: &crate::osc::FixtureStateEmitter) {
                #lines
            }
        }
    }
    .into()
}

/// Derive the Control trait on a fixture struct.
///
/// Fields that do not have a control method can be skipped with #[skip_control].
/// Fields that implement OscControl as well as Control can be forced to emit
/// with the OscControl method with the #[force_osc_control] attribute.
///
/// Fields annotated with #[channel_control] will be wired up to the channel
/// control method.
///
/// Fields annotated with #[animate] will result in a variant in a generated
/// AnimationTarget type. The name of the animation variant will be the
/// PascalCase version of the struct field identifier.
///
/// Fields may declare a named method on the implementing struct to call when
/// a change happens to the control.
///
/// Fields that may be absent (defined as an Option) can set #[optional] to
/// conditionally handle if Some.
#[proc_macro_derive(
    Control,
    attributes(
        skip_control,
        force_osc_control,
        channel_control,
        animate,
        on_change,
        optional
    )
)]
pub fn derive_control(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(input as DeriveInput);

    let Data::Struct(struct_data) = data else {
        panic!("Can only derive Control for structs.");
    };
    let Fields::Named(fields) = struct_data.fields else {
        panic!("Can only derive Control for named structs.");
    };
    let mut control_lines = quote! {};
    let mut channel_control_lines = quote! {};

    let mut animate_target_idents = vec![];

    for field in fields.named.iter() {
        if field_has_attr(field, "skip_control") {
            continue;
        }
        let Some(ident) = &field.ident else {
            continue;
        };

        let on_change = get_attr_and_payload(&field.attrs, "on_change")
            .map(|method| {
                let method = format_ident!("{method}");
                quote! {
                    self.#method(emitter);
                }
            })
            .unwrap_or_default();

        // We'll bind the field mutably to a local named #ident.
        let control_call = if field_has_attr(field, "force_osc_control") {
            quote! {
                if crate::fixture::control::OscControl::control(#ident, msg, emitter)? {
                    #on_change
                    return Ok(true);
                }
            }
        } else {
            quote! {
                if #ident.control(msg, emitter)? {
                    #on_change
                    return Ok(true);
                }
            }
        };

        let optional = field_has_attr(field, "optional");

        control_lines = insert_optional_call(optional, true, ident, control_call, control_lines);

        if field_has_attr(field, "channel_control") {
            let channel_control_call = quote! {
                if #ident.control_from_channel(msg, emitter)? {
                    #on_change
                    return Ok(true);
                }
            };

            channel_control_lines = insert_optional_call(
                optional,
                true,
                ident,
                channel_control_call,
                channel_control_lines,
            );
        }

        if field_has_attr(field, "animate") {
            animate_target_idents.push(ident.to_string().to_case(Case::Pascal));
        }
    }

    let mut anim_target_enum = quote! {};
    if !animate_target_idents.is_empty() {
        for ident in animate_target_idents {
            let ident = format_ident!("{ident}");
            anim_target_enum = quote! {
                #anim_target_enum
                #ident,
            }
        }
        anim_target_enum = quote! {
            #[derive(
                Clone,
                Copy,
                Debug,
                Default,
                PartialEq,
                strum_macros::EnumString,
                strum_macros::EnumIter,
                strum_macros::Display,
                num_derive::FromPrimitive,
                num_derive::ToPrimitive,
            )]
            pub enum AnimationTarget {
                #[default]
                #anim_target_enum
            }
        }
    }

    quote! {
        impl crate::fixture::Control for #ident {
            fn control(&mut self, msg: &crate::osc::OscControlMessage, emitter: &crate::osc::FixtureStateEmitter) -> anyhow::Result<bool> {
                #control_lines
                Ok(false)
            }

            fn control_from_channel(
                &mut self,
                msg: &crate::channel::ChannelControlMessage,
                emitter: &crate::osc::FixtureStateEmitter,
            ) -> anyhow::Result<bool> {
                #channel_control_lines
                Ok(false)
            }
        }

        #anim_target_enum
    }
    .into()
}

fn insert_optional_call(
    optional: bool,
    mutable: bool,
    ident: &Ident,
    call: proc_macro2::TokenStream,
    into: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let ref_token = if mutable { quote!(&mut) } else { quote!(&) };
    if optional {
        return quote! {
            #into
            if let Some(#ident) = #ref_token self.#ident {
                #call
            }
        };
    }
    quote! {
        #into
        {
            let #ident = #ref_token self.#ident;
            #call
        }
    }
}

fn field_has_attr(field: &Field, ident: &str) -> bool {
    field
        .attrs
        .iter()
        .any(|attr| attr.meta.path().is_ident(ident))
}

fn get_attr_and_payload(attrs: &[Attribute], ident: &str) -> Option<String> {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.meta.path().is_ident(ident) {
                return None;
            }
            let Meta::NameValue(nm) = &attr.meta else {
                panic!("attribute {ident} must be name/value, not {:?}", attr.meta);
            };
            let Expr::Lit(f) = &nm.value else {
                panic!("attribute {ident} expected a literal as argument");
            };
            let Lit::Str(s) = &f.lit else {
                panic!("attribute {ident} expected a string literal as argument");
            };
            Some(s.value())
        })
        .next()
}

fn get_attr_and_usize_payload(attrs: &[Attribute], ident: &str) -> Option<usize> {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.meta.path().is_ident(ident) {
                return None;
            }
            let Meta::NameValue(nm) = &attr.meta else {
                panic!("attribute {ident} must be name/value, not {:?}", attr.meta);
            };
            let Expr::Lit(f) = &nm.value else {
                panic!("attribute {ident} expected a literal as argument");
            };
            let Lit::Int(s) = &f.lit else {
                panic!("attribute {ident} expected a integer literal as argument");
            };
            let Ok(val) = s.base10_parse() else {
                panic!("attribute {ident} unable to parse as usize");
            };
            Some(val)
        })
        .next()
}
