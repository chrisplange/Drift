//! Validates the canvas shader and checks that every entry point translates to GLSL ES 3.00,
//! the shading language of WebGL2. This catches WebGPU-only features without needing a browser.

use naga::back::glsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};

const SOURCE: &str = include_str!("../src/canvas.wgsl");

const ENTRY_POINTS: &[(&str, naga::ShaderStage)] = &[
    ("vs_grid", naga::ShaderStage::Vertex),
    ("fs_grid", naga::ShaderStage::Fragment),
    ("vs_segment", naga::ShaderStage::Vertex),
    ("fs_segment", naga::ShaderStage::Fragment),
    ("vs_arc", naga::ShaderStage::Vertex),
    ("fs_arc", naga::ShaderStage::Fragment),
];

fn parse_and_validate() -> (naga::Module, naga::valid::ModuleInfo) {
    let module = naga::front::wgsl::parse_str(SOURCE).unwrap_or_else(|e| {
        panic!("{}", e.emit_to_string(SOURCE));
    });
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SOURCE)));
    (module, info)
}

#[test]
fn shader_is_valid_wgsl() {
    let (module, _) = parse_and_validate();
    for (name, stage) in ENTRY_POINTS {
        assert!(
            module
                .entry_points
                .iter()
                .any(|ep| ep.name == *name && ep.stage == *stage),
            "missing entry point {name}"
        );
    }
}

#[test]
fn shader_translates_to_webgl2() {
    let (module, info) = parse_and_validate();
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..Default::default()
    };
    for (name, stage) in ENTRY_POINTS {
        let pipeline_options = glsl::PipelineOptions {
            shader_stage: *stage,
            entry_point: (*name).to_owned(),
            multiview: None,
        };
        let mut out = String::new();
        let mut writer = glsl::Writer::new(
            &mut out,
            &module,
            &info,
            &options,
            &pipeline_options,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        writer.write().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(out.starts_with("#version 300 es"), "{name}");
    }
}
