
from launch import LaunchDescription
from launch_ros.actions import Node


def generate_launch_description():
    """Talker example that launches everything locally."""
    ld = LaunchDescription()


    # listener_node = Node(
    #     package="bench", executable="talker", output="screen"
    # )

    # ld.add_action(listener_node)

    service_node = Node(
        package="bench", executable="parking_subscriber",
    )

    ld.add_action(service_node)

    sgc_router = Node(
        package="sgc_launch",
        executable="sgc_router", 
        output="screen",
        emulate_tty = True,
        parameters = [
            # find and add config file in ./sgc_launhc/configs
            # or use the `config_file_name` optional parameter
            {"config_file_name": "parking.yaml"}, 
            {"whoami": "machine_listener"},
            {"release_mode": False}
        ]
    )
    ld.add_action(sgc_router)
    
    return ld
