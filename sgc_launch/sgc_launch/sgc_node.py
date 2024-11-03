import subprocess, os, yaml
import requests
import pprint
from time import sleep
import rclpy
import rclpy.node
from rcl_interfaces.msg import SetParametersResult
from sgc_msgs.srv import SgcAssignment
from sgc_msgs.msg import AssignmentUpdate
from .web_requests import *
from .sgc_swarm import SGC_Swarm

logger = None


class SGC_Router_Node(rclpy.node.Node):
    def __init__(self):
        super().__init__("sgc_launch_node")
        self.logger = self.get_logger()
        global logger
        logger = self.logger

        self.declare_parameter("whoami", "")
        self.whoami = self.get_parameter("whoami").value

        self.declare_parameter("config_path", "")
        self.config_path = self.get_parameter("config_path").value

        self.declare_parameter(
            "sgc_base_port", 3000
        )  # port = base_port + ROS_DOMAIN_ID
        self.sgc_base_port = self.get_parameter("sgc_base_port").value

        self.declare_parameter(
            "config_file_name", "PARAMETER NOT SET"
        )  # ROS2 parameter is strongly typed
        self.config_file_name = self.get_parameter("config_file_name").value

        self.ros_domain_id = (
            os.getenv("ROS_DOMAIN_ID") if os.getenv("ROS_DOMAIN_ID") else 0
        )
        self.sgc_router_api_port = self.sgc_base_port + int(self.ros_domain_id)
        self.sgc_router_api_addr = f"localhost:{self.sgc_router_api_port}"
        self.swarm = None

        if self.config_file_name == "PARAMETER NOT SET":

            self.logger.warn(
                "No config file specified! Use unstable automatic topic discovery!"
            )
            self.timer = self.create_timer(1, self.discovery_callback)
            self.automatic_mode = True
        else:
            self.automatic_mode = False

        self.declare_parameter("release_mode", True)
        self.release_mode = self.get_parameter("release_mode").value

        self.launch_sgc(
            self.config_path,
            self.config_file_name,
            self.logger,
            self.whoami,
            self.release_mode,
            self.automatic_mode,
        )

        self.discovered_topics = [
            "/rosout",
            "/parameter_events",
        ]  # we shouldn't expose them as global topics
        self.add_on_set_parameters_callback(self.parameters_callback)

        self.assignment_server = self.create_service(
            SgcAssignment, "sgc_assignment_service", self.sgc_assignment_callback
        )

        self.assignment_update_publisher = self.create_publisher(
            AssignmentUpdate, "fogros_sgc/assignment_update", 10
        )

        # subscribe to the profile topic from other machines (if any)
        # for now we assume only one machine (i.e. robot) can issue the command, and use sgc
        # to propagate to other machines
        # later we can make it dynamic
        self.assignment_update_subscriber = self.create_subscription(
            AssignmentUpdate,
            "fogros_sgc/assignment_update",
            self.assignment_update_callback,
            10,
        )

    def parameters_callback(self, params):
        self.logger.info(f"got {params}!!!")
        return SetParametersResult(successful=False)

    # ros2 service call /sgc_assignment_service sgc_msgs/srv/SgcAssignment '{machine: {data: "a"}, state: {data: "b"} }'
    def sgc_assignment_callback(self, request, response):
        machine = request.machine.data
        state = request.state.data
        assignment_dict = {machine: state}
        self.swarm.apply_assignment(assignment_dict)
        update = AssignmentUpdate()
        update.machine = request.machine
        update.state = request.state
        # republish
        self.assignment_update_publisher.publish(update)
        self.logger.warn(f"successfully published the update {update}")
        response.result.data = "success"
        return response

    def assignment_update_callback(self, update):
        machine = update.machine.data
        state = update.state.data
        if self.swarm.assignment_dict[machine] == state:
            self.logger.warn(f"the update {update} is the same, not updating")
        else:
            assignment_dict = {machine: state}
            self.swarm.apply_assignment(assignment_dict)
            self.logger.warn(
                f"the updated machine assignment is {self.swarm.assignment_dict}"
            )
            # republishing (is this needed)
            # self.assignment_update_publisher.publish(update)

    def launch_sgc(
        self,
        config_path,
        config_file_name,
        logger,
        whoami,
        release_mode,
        automatic_mode,
    ):
        current_env = os.environ.copy()
        current_env["PATH"] = f"/usr/sbin:/sbin:{current_env['PATH']}"
        ws_path = current_env["COLCON_PREFIX_PATH"]
        # source directory of sgc
        sgc_path = f"{ws_path}/../rt-fogros2/fog_rs"
        # directory of all the config files
        config_path = (
            f"{ws_path}/sgc_launch/share/sgc_launch/configs"
            if not config_path
            else config_path
        )
        # directory of all the crypto files
        crypto_path = f"{ws_path}/sgc_launch/share/sgc_launch/configs/crypto/test_cert/test_cert-private.pem"
        # check if the crypto files are generated, if not, generate them
        if not os.path.isfile(crypto_path):
            logger.info(f"crypto file does not exist in {crypto_path}, generating...")
            subprocess.call(
                [
                    f"cd {ws_path}/sgc_launch/share/sgc_launch/configs && ./generate_crypto.sh"
                ],
                shell=True,
            )

        # setup crypto path
        current_env["SGC_CRYPTO_PATH"] = f"{crypto_path}"

        if automatic_mode:
            logger.info("automatic discovery is enabled")
        else:
            config_path = f"{config_path}/{config_file_name}"
            logger.info(f"using yaml config file {config_path}")
            self.swarm = SGC_Swarm(
                config_path, whoami, logger, self.sgc_router_api_addr
            )

        current_env["SGC_CONFIG_PATH"] = f"{config_path}"

        # build and run SGC
        logger.info("building FogROS SGC... It takes longer for first time")
        # remove the stale SGC router if the domain id is the same
        grep_result = subprocess.run(
            f"lsof -i -P -n | grep LISTEN | grep {self.sgc_router_api_port}",
            env=current_env,
            capture_output=True,
            text=True,
            shell=True,
        ).stdout
        if grep_result:
            logger.warn(
                "Previous run of SGC router in the same domain is running, killing it..."
            )
            pid = grep_result.split()[1]
            subprocess.call(f"kill {pid}", env=current_env, shell=True)

        # check if the port exists
        current_env["SGC_API_PORT"] = str(self.sgc_router_api_port)
        if self.swarm:
            current_env["SGC_SIGNAL_SERVER_ADDRESS"] = (
                self.swarm.signaling_server_address
            )
            current_env["SGC_RIB_SERVER_ADDRESS"] = (
                self.swarm.routing_information_base_address
            )
            self.logger.info(
                f"using signaling server address {self.swarm.signaling_server_address}, routing information base address {self.swarm.routing_information_base_address}"
            )
        else:
            current_env["SGC_SIGNAL_SERVER_ADDRESS"] = "127.0.0.1:8000"
            self.logger.info(
                f"using default signaling server address {current_env['SGC_SIGNAL_SERVER_ADDRESS']}, routing information base address {current_env['SGC_RIB_SERVER_ADDRESS']}"
            )

        # if os.path.isfile(f"{sgc_path}/target/debug/gdp-router") or os.path.isfile(f"{sgc_path}/target/release/gdp-router"):
        #     logger.info("previous build of SGC router exists, skipping build")

        # else:
        logger.info("building SGC router...")
        if release_mode:
            subprocess.call(
                f"cargo build --release --manifest-path {sgc_path}/Cargo.toml",
                env=current_env,
                shell=True,
            )
        else:
            subprocess.call(
                f"cargo build --manifest-path {sgc_path}/Cargo.toml",
                env=current_env,
                shell=True,
            )

        logger.info("running SGC router...")
        if release_mode:
            subprocess.Popen(
                f"{sgc_path}/target/release/gdp-router router",
                env=current_env,
                shell=True,
            )
        else:
            subprocess.Popen(
                f"{sgc_path}/target/debug/gdp-router router",
                env=current_env,
                shell=True,
            )

        # if release_mode:
        #     # subprocess.call(f"cargo build --release --manifest-path {sgc_path}/Cargo.toml", env=current_env,  shell=True)
        #     subprocess.Popen(f"{sgc_path}/target/release/gdp-router router", env=current_env,  shell=True)
        # else:
        #     # subprocess.call(f"cargo build --manifest-path {sgc_path}/Cargo.toml", env=current_env,  shell=True)
        #     subprocess.Popen(f"{sgc_path}/target/release/gdp-router router", env=current_env,  shell=True)

        if not self.automatic_mode:
            sleep(5)
            self.swarm.apply_assignment(
                self.swarm.get_assignment_from_yaml(config_path)
            )

    def discovery_callback(self):
        current_env = os.environ.copy()
        output = subprocess.run(
            f"ros2 topic list -t",
            env=current_env,
            capture_output=True,
            text=True,
            shell=True,
        ).stdout
        for line in output.split("\n"):
            # /rosout [rcl_interfaces/msg/Log]
            if line == "":
                continue
            topic_name = line.split(" ")[0]
            topic_type = line.split(" ")[1][1:-1]
            if topic_name not in self.discovered_topics:
                self.get_logger().info(f"found a new topic {topic_name} {topic_type}")
                output = subprocess.run(
                    f"ros2 topic info {topic_name}",
                    env=current_env,
                    capture_output=True,
                    text=True,
                    shell=True,
                ).stdout
                if "Subscription count: 0" in output:
                    self.get_logger().info(
                        f"publish {topic_name} as a remote publisher"
                    )
                    topic_action = "pub"
                    send_topic_request(
                        "add",
                        topic_action,
                        topic_name,
                        topic_type,
                        self.sgc_router_api_addr,
                    )
                elif "Publisher count: 0" in output:
                    self.get_logger().info(
                        f"publish {topic_name} as a remote subscriber"
                    )
                    topic_action = "sub"
                    send_topic_request(
                        "add",
                        topic_action,
                        topic_name,
                        topic_type,
                        self.sgc_router_api_addr,
                    )
                else:
                    self.get_logger().info(f"cannot determine {topic_name} direction")
                self.discovered_topics.append(topic_name)
            # TODO: remove a topic when the topic is gone


def main():
    rclpy.init()
    node = SGC_Router_Node()
    rclpy.spin(node)


if __name__ == "__main__":
    main()
