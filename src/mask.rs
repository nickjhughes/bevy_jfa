use bevy::{
    pbr::{MeshPipeline, MeshPipelineKey, MAX_CASCADES_PER_LIGHT, MAX_DIRECTIONAL_LIGHTS},
    prelude::*,
    render::{
        mesh::InnerMeshVertexBufferLayout,
        render_graph::{Node, RenderGraphContext, SlotInfo, SlotType},
        render_phase::RenderPhase,
        render_resource::{
            ColorTargetState, ColorWrites, FragmentState, LoadOp, MultisampleState, Operations,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
            ShaderDefVal, SpecializedMeshPipeline, SpecializedMeshPipelineError, TextureFormat,
        },
        renderer::RenderContext,
    },
    utils::{FixedState, Hashed},
};

use crate::{resources::OutlineResources, MeshMask, MASK_SHADER_HANDLE};

#[derive(Resource)]
pub struct MeshMaskPipeline {
    mesh_pipeline: MeshPipeline,
}

impl FromWorld for MeshMaskPipeline {
    fn from_world(world: &mut World) -> Self {
        let mesh_pipeline = world.get_resource::<MeshPipeline>().unwrap().clone();

        MeshMaskPipeline { mesh_pipeline }
    }
}

impl SpecializedMeshPipeline for MeshMaskPipeline {
    type Key = MeshPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &Hashed<InnerMeshVertexBufferLayout, FixedState>,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mut desc = self.mesh_pipeline.specialize(key, layout)?;

        desc.layout = vec![
            if key.msaa_samples() > 0 {
                self.mesh_pipeline.view_layout_multisampled.clone()
            } else {
                self.mesh_pipeline.view_layout.clone()
            },
            self.mesh_pipeline.mesh_layout.clone(),
        ];

        desc.vertex.shader = MASK_SHADER_HANDLE.typed::<Shader>();

        desc.fragment = Some(FragmentState {
            shader: MASK_SHADER_HANDLE.typed::<Shader>(),
            shader_defs: vec![
                ShaderDefVal::Int(
                    "MAX_DIRECTIONAL_LIGHTS".to_string(),
                    MAX_DIRECTIONAL_LIGHTS as i32,
                ),
                ShaderDefVal::Int(
                    "MAX_CASCADES_PER_LIGHT".to_string(),
                    MAX_CASCADES_PER_LIGHT as i32,
                ),
            ],
            entry_point: "fragment".into(),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::R8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        });
        desc.depth_stencil = None;

        desc.multisample = MultisampleState {
            count: 4,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };

        desc.label = Some("mesh_stencil_pipeline".into());
        Ok(desc)
    }
}

/// Render graph node for producing stencils from meshes.
pub struct MeshMaskNode {
    query: QueryState<&'static RenderPhase<MeshMask>>,
}

impl MeshMaskNode {
    pub const IN_VIEW: &'static str = "view";

    /// The produced stencil buffer.
    ///
    /// This has format `TextureFormat::Depth24PlusStencil8`. Fragments covered
    /// by a mesh are assigned a value of 255. All other fragments are assigned
    /// a value of 0. The depth aspect is unused.
    pub const OUT_MASK: &'static str = "stencil";

    pub fn new(world: &mut World) -> MeshMaskNode {
        MeshMaskNode {
            query: QueryState::new(world),
        }
    }
}

impl Node for MeshMaskNode {
    fn input(&self) -> Vec<SlotInfo> {
        vec![SlotInfo::new(Self::IN_VIEW, SlotType::Entity)]
    }

    fn output(&self) -> Vec<SlotInfo> {
        vec![SlotInfo::new(Self::OUT_MASK, SlotType::TextureView)]
    }

    fn update(&mut self, world: &mut World) {
        self.query.update_archetypes(world);
    }

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), bevy::render::render_graph::NodeRunError> {
        let res = world.get_resource::<OutlineResources>().unwrap();

        graph
            .set_output(Self::OUT_MASK, res.mask_multisample.default_view.clone())
            .unwrap();

        let view_entity = graph.get_input_entity(Self::IN_VIEW).unwrap();
        let stencil_phase = match self.query.get_manual(world, view_entity) {
            Ok(q) => q,
            Err(_) => return Ok(()),
        };

        let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("outline_stencil_render_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &res.mask_multisample.default_view,
                resolve_target: Some(&res.mask_output.default_view),
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK.into()),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });

        stencil_phase.render(&mut pass, world, view_entity);

        Ok(())
    }
}
