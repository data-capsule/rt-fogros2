#![no_std]
#![no_main]

use aya_ebpf::{
    bindings::{TC_ACT_OK, TC_ACT_PIPE, TC_ACT_SHOT}, helpers::{bpf_clone_redirect, bpf_csum_diff}, macros::{classifier, map}, maps::HashMap, programs::TcContext
};

use aya_log_ebpf::{info, warn};
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
    tcp::TcpHdr,
    udp::{self, UdpHdr},
};
use core::{mem, net::Ipv4Addr};
mod utils;

use crate::{
    utils::{csum_fold_helper, ptr_at},
};


#[map]
static BLOCKLIST: HashMap<u32, u32> = HashMap::with_max_entries(1024, 0);

#[classifier]
pub fn tc_egress(ctx: TcContext) -> i32 {
    match try_tc_egress(ctx) {
        Ok(ret) => ret,
        Err(_) => TC_ACT_SHOT,
    }
}


fn block_ip(address: u32) -> bool {
    unsafe { BLOCKLIST.get(&address).is_some() }
}

fn try_tc_egress(ctx: TcContext) -> Result<i32, i64> {
    // let ethhdr: EthHdr = ctx.load(0).map_err(|_| ())?;
    let ethhdr: *mut EthHdr = unsafe { ptr_at(&ctx, 0)?};
    match unsafe{(*ethhdr).ether_type} {
        EtherType::Ipv4 => {}
        _ => return Ok(TC_ACT_PIPE),
    }

    let ip_hdr: *mut Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)?};
    let destination = u32::from_be( unsafe {
        (*ip_hdr).dst_addr
    });

    let action = if block_ip(destination) {

        let ifindex = 2; // Target interface index

        // modify the udp port 
        // info!(&ctx, "{}",ctx.data());
        
        let udp_hdr: *mut UdpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN + Ipv4Hdr::LEN)?};
        unsafe {
            // address of 127.0.0.1
            // https://www.browserling.com/tools/ip-to-dec
            (*ip_hdr).dst_addr = Ipv4Addr::new(54, 153, 114, 6).into();  //54.153.114.6
            (*ip_hdr).check = 0;
        }
        let full_cksum = unsafe {
            bpf_csum_diff(
                mem::MaybeUninit::zeroed().assume_init(),
                0,
                ip_hdr as *mut u32,
                Ipv4Hdr::LEN as u32,
                0,
            )
        } as u64;
        unsafe { (*ip_hdr).check = csum_fold_helper(full_cksum) };

        unsafe{
            (*udp_hdr).check = 0;
        }


        let _ = &ctx.clone_redirect(ifindex, 0).map_err(
            |err| (
            )
        );

        let ip_hdr_2: *mut Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)?};
        let udp_hdr_2: *mut UdpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN + Ipv4Hdr::LEN)?};
        unsafe {
            (*ip_hdr_2).dst_addr = Ipv4Addr::new(6, 114, 153, 54).into();   //54.153.114.6
            (*ip_hdr_2).check = 0;
        }
        let full_cksum = unsafe {
            bpf_csum_diff(
                mem::MaybeUninit::zeroed().assume_init(),
                0,
                ip_hdr_2 as *mut u32,
                Ipv4Hdr::LEN as u32,
                0,
            )
        } as u64;
        unsafe { (*ip_hdr_2).check = csum_fold_helper(full_cksum) };

        unsafe{
            (*udp_hdr_2).check = 0;
        }
        let _ = &ctx.clone_redirect(ifindex, 0).map_err(
            |err| (
            )
        );


        // let _ = &ctx.clone_redirect(ifindex, 0).map_err(
        //     |err| (
        //     )
        // );

        // change the destination and send to another host 
        // bpf_clone_redirect(ctx.if

        TC_ACT_SHOT
    } else {
        TC_ACT_PIPE
    };

    if action == TC_ACT_SHOT {
        info!(&ctx, "DEST {:i}, ACTION {}", destination, action);
    }

    Ok(action)
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
