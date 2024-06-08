# first build an image with rust, ros and cargo chef
FROM osrf/ros:humble-desktop AS chef
RUN apt update && apt install -y build-essential curl pkg-config libssl-dev protobuf-compiler clang python3-pip
RUN pip install requests
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /fog_ws 
# changed to .
COPY . /fog_ws/rt-fogros2
RUN . /opt/ros/humble/setup.sh && colcon build 