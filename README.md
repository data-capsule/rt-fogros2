# Reliable FogROS2

Reliable FogROS2 is a cloud robotics platform on FogROS2-SGC for connecting disjoint ROS2 networks across different physical locations, networks, and Data Distribution Services. 


\[[Website](https://sites.google.com/view/fogros2-sgc)\] \[[Video](https://youtu.be/hVVFVGLcK0c)\] \[[Arxiv](https://arxiv.org/abs/2306.17157)\]

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->
**Table of Contents**

- [Local Demo](#local-demo)
- [Build FogROS2 SGC](#build-fogros2-sgc)
  - [Install dependencies](#install-dependencies)
    - [Install Rust](#install-rust)
    - [Install ROS](#install-ros)
  - [Build the repo](#build-the-repo)
- [Run with Different Machines](#run-with-different-machines)
    - [Certificate Generation](#certificate-generation)
    - [Run ROS2 talker and listener](#run-ros2-talker-and-listener)
    - [Run with Environment Variables](#run-with-environment-variables)
- [From SGC to SGC-lite](#from-sgc-to-sgc-lite)
    - [Why Lite version](#why-lite-version)
    - [Deploying Your Own Routing Infrastructure](#deploying-your-own-routing-infrastructure)
    - [Notes on using Berkeley's Public Servers](#notes-on-using-berkeleys-public-servers)
    - [TODOs](#todos)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->


## (Recommended) Docker Setup

#### Step 0: Generate Crypto Certificate 

The certificates can be generated by 
```
cd sgc_launch/configs
./generate_crypto.sh
```
You need to make sure all the machines that you intend to connect share the same certificate. 

You can check if the certificates match by running
```
cat /fog_ws/install/sgc_launch/share/sgc_launch/configs/crypto/test_cert/test_cert-private.pem
```
The default certificate is named `test_cert`. 

#### Step 1: Build Container
Build container with the following command:
```bash
docker build --build-arg USER_ID=$(id -u) --build-arg GROUP_ID=$(id -g) --build-arg USER_NAME="$USER"  -t dev-fr .
```
then start **two** terminals, for both, run 
```bash
docker run --user $(id -u):$(id -g) -ti --net=host -v ~/rt-fogros2:/fog_ws/rt-fogros2  dev-fr bash 
```
The parameters such as user ID and group are used to make sure the built targets are readable and modifiable by the host. 

### Run Hello World Example 
```
RUSTFLAGS="--cfg tokio_unstable"  cargo run
```
On each terminal, run 
```
export RUST_LOG=info && export ROS_DOMAIN_ID=3 && source install/setup.bash && ros2 launch bench benchmark.talker.launch.py 
```
and 
```
export RUST_LOG=info && export ROS_DOMAIN_ID=2 && source install/setup.bash && ros2 launch bench benchmark.listener.launch.py 
```

### F&Q
1. If the lisenter side is not responsive (no logs show up) when the talker launches, double check the certificate. Sometimes it gets tricky with `/r/n` and `/n` or whether the file ends with a newline character (the editor may do something stupid). Double check with 
```
od -c /fog_ws/install/sgc_launch/share/sgc_launch/configs/crypto/test_cert/test_cert-private.pem
```

2. If the listner is responsive but they don't connect, check the internet firewall rules. We require one side of the machines are connectable via UDP (LMK if you want specific ports)

3. too many redis logs. Currently we haven't implemented automatic cleanup, but you can cleanup manually via 
```
redis-cli -h 3.18.194.127 -p 8002
> FLUSHDB
```


## Setup FogROS2 SGC 
The following are instructions of setting up FogROS2 SGC. 

### Install dependencies 
```
sudo apt update
sudo apt install build-essential curl pkg-config libssl-dev protobuf-compiler clang
```

#### Install Rust 
```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

#### Install ROS 
ROS2 ~Dashing~ ~Eloquent~ Foxy Galactic Humble Rolling should work fine with FogROS2 SGC. 

Here we show the instruction of installing ROS2 rolling with Debian packages. 

First, install ROS2 from binaries with [these instructions](https://docs.ros.org/en/rolling/Installation/Ubuntu-Install-Debians.html).

Setup your environment with [these instructions](https://docs.ros.org/en/rolling/Installation/Ubuntu-Install-Debians.html#environment-setup).

Every terminal should be configured with 
```
source /opt/ros/rolling/setup.bash
```

If you have custom types in a specific ROS colcon directory, `source` the `setup.bash` in that directory. 

#### Certificate Generation
Either you run it as a ROS node, or as a standalone process, you need to generate a certificate unique to the task as the globally unique identifier. This set of certificate should be shared by all the robots that you wish to be connected. 

The certificates can be generated by 
```
cd sgc_launch/configs
./generate_crypto.sh
```
Every directory in `./sgc_launch/configs/crypto` contains the cryptographic secrets needed for communication. 

Distribute the `crypto` directory by from machine A and machine B. This can be done with USB copying, typing or SSH. Here is an example with `scp`: 
```
scp -r crypto USER@MACHINE_B_IP_ADDR:/SGC_PATH/sgc_launch/configs/
```
replace `USER`, `MACHINE_B_IP_ADDR`, `SGC_PATH` with the actual paths.

After the crypto secrets are delivered, go back to project main directory. 


## (Recommended) Run as a ROS2 node

#### Step1: Create a ROS workspace and clone this repo to the workspace and build it
```
cd ~
mkdir -p fog_ws/src
cd ~/fog_ws/src
git clone https://github.com/KeplerC/fogros2-sgc.git

cd ~/fog_ws 
colcon build
```
Please make sure that the repo is cloned directly under the `src` directory. 

#### Step2: Run with ros2 launch  
Machine A: 
```
ros2 launch sgc_launch talker.launch.py
```

Machine B: 
```
ros2 launch sgc_launch listener.launch.py
```
Note that machine A and machine B do not need to configure any IP address, and they automatically connect. 

Please refer to the  [README](./sgc_launch/README.md) for writing the launch file for your application. FogROS2-SGC supports automatic topic discovery, but it is recommended to expose the public interface only if intended. 

## Run as a standalone process
### Build and Run

The repo can be built with 
```
cargo run router
```
If you want to deploy with production system, use `cargo build --release` option for optimization level and removal of debug logs. 

To disable the logs and run with high optimization, run with `release` option by 
`
cargo run --release router
`
instead.

Adding and removing topics requires REST API. Example of such is 
```
curl -X POST http://localhost:3000/topic \
   -H 'Content-Type: application/json' \
   -d '{"api_op":"add","ros_op":"pub", "crypto":"test_cert", "topic_name":"/chatter", "topic_type":"std_msgs/msg/String"}'
```
The port for the REST interface can be changed via `SGC_API_PORT` environment variable.

## Notes 
The configuration is currently coded with a Berkeley's signaling server that facilitates the routing inforamtion exchange. See [Making your own signaling server](#making-your-own-signaling-server) section for creating your own signaling server.
The system should also work if you don't specify the configuration file, then it uses automatic mode that 
constantly checking for new topics. We note that it is only a convenient interface and not FogROS2-SGC is designed for.
As long as the talker and listener use the same crypto, the system should work.

#### How is it different from IROS 2023 version
In the updated version, we removed all the setup about protocols, ports, ips and gateways.
Previous FogROS2-SGC carries a bag of protocols to support heterogenous demands and requirements. 
In this version, we streamline the routing setup by [webrtc](./docs/webrtc.md) instead of building all protocols with raw DTLS sockets.
webrtc is generally not compatible with the previous protocol. 

#### Notes on using Berkeley's Public Servers
Berkeley's public servers are for experimental purposes and do not intend to be used for production system. We do not provide any guarantee on avaialbility. Please use your own signaling server for benchmarking and deployment.
The security guarnatees of FogROS2-SGC prevents other users/Berkeley from learning sensitive information, such as your ROS2 topic name and type, and on the actual data payload. What is visible is a random 256 bit name are published and subscribed by other random 256 bit names. 

#### Deploying Your Own Routing Infrastructure
If you don't want to use Berkeley's infrastructure, having one on your own is very easy. 
This can be done by running 
```
docker compose up -d signal rib
```
The only requirement is to expose port 8000 and 8002 to other robots/machines. 

Signaling server faciliates the communication by exchanging the address information of webrtc. The details about how signaling server works can be found [HERE](./docs/webrtc.md).
Then update the config files by replacing the IP address to your server's IP address.  



#### TODOs 
1. expiration time for stale keys (this may happen if the subscriber suddenly drops off and does not connect to an existing publisher)
2. in some rare cases, the IP address and port provided cannot connect, which blocks the publisher and subscriber. The common root cause is that the firewall bans the ports larger than 50000, which may happen in some enterprise or restrictive settings. If you have one side of the machine (like cloud) that opens the port greater than 50000, it should be able to connect. 
