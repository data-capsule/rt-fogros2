use std::{env, net::Ipv4Addr};


// address used to filter the packets
pub fn get_non_existent_ip_addr() -> Ipv4Addr {
    Ipv4Addr::new(192, 168, 123, 234)
}

// read from environment variable signaling server address
pub fn get_signaling_server_address() -> String {
    match env::var_os("SGC_SIGNAL_SERVER_ADDRESS") {
        Some(address) => address.into_string().unwrap(),
        None => {
            info!("Using default signaling server address");
            "20.172.64.185:8000".to_owned()
        }
    }
}

// read from environment variable rib address
pub fn get_rib_server_address() -> String {
    match env::var_os("SGC_RIB_SERVER_ADDRESS") {
        Some(address) => address.into_string().unwrap(),
        None => {
            info!("Using default rib server address");
            "3.18.194.127:8002".to_owned()
        }
    }
}
