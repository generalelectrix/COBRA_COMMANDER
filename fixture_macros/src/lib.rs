use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Attribute, Data, DeriveInput, Expr, Field, Fields, Lit, Meta};

/// Derive the PatchAnimatedFixture trait on a fixture struct.
/// Use the channel_count attribute to specify the DMX channel count.
/// Registers the fixture type with the patch.
#[proc_macro_derive(PatchAnimatedFixture, attributes(channel_count))]
pub fn derive_patch_animated_fixture(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, attrs, .. } = parse_macro_input!(input as DeriveInput);

    let channel_count = get_attr_and_usize_payload(&attrs, "channel_count")
        .expect("channel_count attribute is missing");

    let name = ident.to_string();

    quote! {
        impl crate::fixture::patch::PatchAnimatedFixture for #ident {
            const NAME: FixtureType = FixtureType(#name);
            fn channel_count(&self) -> usize {
                #channel_count
            }
        }

        crate::register!(#ident);
    }
    .into()
}

/// Derive the PatchFixture trait on a fixture struct.
/// Use the channel_count attribute to specify the DMX channel count.
/// Registers the fixture type with the patch.
#[proc_macro_derive(PatchFixture, attributes(channel_count))]
pub fn derive_patch_fixture(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, attrs, .. } = parse_macro_input!(input as DeriveInput);

    let channel_count = get_attr_and_usize_payload(&attrs, "channel_count")
        .expect("channel_count attribute is missing");

    let name = ident.to_string();

    quote! {
        impl crate::fixture::patch::PatchFixture for #ident {
            const NAME: FixtureType = FixtureType(#name);
            fn channel_count(&self) -> usize {
                #channel_count
            }
        }

        crate::register!(#ident);
    }
    .into()
}

/// Derive the EmitState trait on a fixture struct.
///
/// Fields that do not have an emit_state method can be skipped with #[skip_emit].
/// Fields that implement OscControl as well as EmitState can be forced to emit
/// with the OscControl method with the #[force_osc_control] attribute.
#[proc_macro_derive(EmitState, attributes(skip_emit, force_osc_control))]
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
        if field_has_attr(field, "force_osc_control") {
            lines = quote! {
                #lines
                crate::fixture::control::OscControl::emit_state(&self.#ident, emitter);
            }
        } else {
            lines = quote! {
                #lines
                self.#ident.emit_state(emitter);
            }
        }
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
#[proc_macro_derive(
    Control,
    attributes(skip_control, force_osc_control, channel_control, animate, on_change)
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
        if field_has_attr(field, "force_osc_control") {
            control_lines = quote! {
                #control_lines
                if crate::fixture::control::OscControl::control(&mut self.#ident, msg, emitter)? {
                    #on_change
                    return Ok(true);
                }
            }
        } else {
            control_lines = quote! {
                #control_lines
                if self.#ident.control(msg, emitter)? {
                    #on_change
                    return Ok(true);
                }
            }
        }
        if field_has_attr(field, "channel_control") {
            channel_control_lines = quote! {
                #channel_control_lines
                if self.#ident.control_from_channel(msg, emitter)? {
                    #on_change
                    return Ok(true);
                }
            }
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
