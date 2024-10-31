# first build an image with rust, ros and cargo chef
FROM ros:humble AS chef


RUN sudo apt update && sudo apt install -y build-essential curl pkg-config libssl-dev protobuf-compiler clang python3-pip ros-humble-rmw-cyclonedds-cpp
RUN pip install requests
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# install realsense dependency
RUN apt install -y ros-humble-librealsense2* ros-humble-realsense2-*
# install rtabmap_sync
RUN  apt update &&  apt install -y ros-humble-rtabmap-ros

WORKDIR /fog_ws 
# changed to .
COPY . /fog_ws/rt-fogros2
RUN . /opt/ros/humble/setup.sh && colcon build 
# --symlink-install

WORKDIR /fog_ws/rt-fogros2/fog_rs
RUN . /fog_ws/install/setup.sh && cargo build --release

WORKDIR /fog_ws/
CMD ["bash"]
# docker build --build-arg USER_ID=$(id -u) --build-arg GROUP_ID=$(id -g)  -t dev-fr .
# docker run -ti --net=host -v ~/rt-fogros2:/fog_ws/rt-fogros2  dev-fr bash  

# docker build -t dev-fr . && docker run -ti --net=host -v ~/rt-fogros2:/fog_ws/rt-fogros2  dev-fr bash 