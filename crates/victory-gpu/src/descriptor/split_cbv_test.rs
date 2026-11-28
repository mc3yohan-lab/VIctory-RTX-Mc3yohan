/// Two host segments use the graphics tables made by the real draw state.
/// A one-thread shader reads the dynamic descriptor that each segment bound.
#[test]
#[ignore = "run with VICTORY_D3D12_WARP=1 and VICTORY_PORT_CBV=0 in an isolated process"]
fn a_split_refreshes_only_tables_that_copied_constants() {
    use crate::host::{compile, transition};
    use victory_shader::{ResourceType, RootType, UsedResource};
    use windows::Win32::Graphics::Direct3D12::*;
    use windows::core::s;

    assert!(
        crate::switch::on("VICTORY_D3D12_WARP"),
        "select WARP for this GPU test"
    );
    assert!(!crate::port_cbv::on(), "exercise the copied CBV path");
    let device = crate::host::host().expect("WARP");
    let desc = HeapDesc {
        kind: VIEWS_KIND,
        count: 2,
        flags: SHADER_VISIBLE,
        nodes: 1,
    };
    let mut h: This = std::ptr::null();
    assert_eq!(
        create(
            desc,
            Placement {
                address: 0,
                size: 0
            },
            &mut h
        ),
        S_OK
    );
    let title = heap(h);
    let views = title.host().unwrap();
    let (cbv, srv) = (title.handle(0).unwrap(), title.handle(1).unwrap());
    const A: u32 = 0x1357_9bdf;
    const B: u32 = 0x2468_ace0;
    let mut source = Box::new([A; 64]);
    let address = source.as_ptr() as u64;
    create_cbv(
        std::ptr::null(),
        &[address as u32, (address >> 32) as u32, 256, 0],
        cbv,
    );
    // An SRV that needs a dynamic table because the shader reads its halves as integers.
    let resource = [1, 0, 0, 0, 256, 0, 1, 0x1_0001, 0, 1, 0, 1, 0, 0];
    let mut words = [0u32; 16];
    words[..10].copy_from_slice(&[10, 1, 5768, 0, 0, 0, 32, 0, 0, 0]);
    words[10] = address as u32;
    words[11] = (address >> 32) as u32;
    create_placed_srv(std::ptr::null(), &resource, &words, srv);
    let entry = |slot, kind| UsedResource {
        kind,
        root: RootType::Table,
        table_offset_dwords: 0,
        lower_bound: 0,
        stride: 8,
        dwords: 4,
        user_data: 0,
        slot,
    };
    let plan = crate::draw::TablePlan::new(
        &[entry(0, ResourceType::Cbv), entry(1, ResourceType::Srv)],
        &[],
    );
    let mut state = crate::draw::State::default();
    state.set_heaps(&[h]);
    state.set_table(false, 0, cbv);
    state.set_table(false, 1, srv);
    let shader = compile(
        r#"
        cbuffer Params : register(b0) { uint value; }
        RWByteAddressBuffer result : register(u0);
        [RootSignature("DescriptorTable(CBV(b0)),UAV(u0)")]
        [numthreads(1,1,1)] void main() { result.Store(0,value); }
    "#,
        s!("main"),
        s!("cs_5_1"),
    )
    .unwrap();
    // SAFETY: embedded root signature and bytecode remain alive through creation.
    let (root, pipeline): (ID3D12RootSignature, ID3D12PipelineState) = unsafe {
        let root = device.device.CreateRootSignature(0, &shader).unwrap();
        let pipeline = device
            .device
            .CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: shader.as_ptr().cast(),
                    BytecodeLength: shader.len(),
                },
                ..Default::default()
            })
            .unwrap();
        (root, pipeline)
    };
    let parts = [device.new_list().unwrap(), device.new_list().unwrap()];
    let read = device.read_back_buffer(8).unwrap();
    let mut outputs = Vec::new();
    let mut slots = Vec::new();
    for (i, list) in parts.iter().enumerate() {
        if i == 1 {
            state.replay(list);
        }
        let made = state.tables(list, &plan, &[(1, 0)], false);
        let [(0, cbv_offset), (1, srv_offset)] = made[..] else {
            panic!("expected two dynamic tables")
        };
        assert_ne!(cbv_offset, u32::MAX);
        assert_ne!(srv_offset, u32::MAX);
        slots.push((cbv_offset, srv_offset));
        let result = device.uav_buffer_internal(4).unwrap();
        // SAFETY: live resources, closed after recording, rooted and sized to their declared types.
        unsafe {
            list.list.ResourceBarrier(&[transition(
                &result,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            )]);
            list.list.SetDescriptorHeaps(&[Some(views.heap.clone())]);
            list.list.SetComputeRootSignature(&root);
            list.list.SetPipelineState(&pipeline);
            list.list
                .SetComputeRootDescriptorTable(0, views.gpu_handle(cbv_offset / 32));
            list.list
                .SetComputeRootUnorderedAccessView(1, result.GetGPUVirtualAddress());
            list.list.Dispatch(1, 1, 1);
            list.list.ResourceBarrier(&[transition(
                &result,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            list.list
                .CopyBufferRegion(&read, i as u64 * 4, &result, 0, 4);
        }
        outputs.push(result);
        list.close();
    }
    device.execute(&[&parts[0]]);
    source[0] = B;
    device.execute(&[&parts[1]]);
    // SAFETY: a completed copy of two words to host-readable storage.
    let values = unsafe {
        let mut mapped = std::ptr::null_mut();
        read.Map(
            0,
            Some(&D3D12_RANGE { Begin: 0, End: 8 }),
            Some(&mut mapped),
        )
        .unwrap();
        let values = mapped.cast::<[u32; 2]>().read_unaligned();
        read.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
        values
    };
    drop(state);
    slot::<extern "system" fn(This) -> u32>(h, 0x10)(h);
    eprintln!("actual graphics tables: {slots:?}, GPU values {values:08x?}");
    assert_eq!(
        values,
        [A, B],
        "each segment must snapshot its own constants at submission"
    );
    assert_ne!(
        slots[0].0, slots[1].0,
        "the second segment needs independent scratch"
    );
    assert_eq!(slots[0].1, slots[1].1, "the SRV-only table is still valid");
}
