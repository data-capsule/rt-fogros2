# first build an image with rust, ros and cargo chef
FROM osrf/ros:humble-desktop AS chef
RUN apt update && apt install -y build-essential curl pkg-config libssl-dev protobuf-compiler clang
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /fog_ws 
# changed to .
COPY ./rt-fogros2 /fog_ws/src/rt-fogros2
RUN . /opt/ros/humble/setup.sh && colcon build 
RUN . ./install/setup.sh &&  cargo build --release --manifest-path ./src/rt-fogros2/Cargo.toml


RUN apt-get update && apt-get install -y python3-pip
RUN pip install requests
RUN ls /fog_ws/install/../src/rt-fogros2/target/release/gdp-router
CMD . ./install/setup.sh && ros2 run sgc_launch sgc_router --ros-args -p config_file_name:=service-client.yaml -p whoami:=service_server -p release_mode:=True