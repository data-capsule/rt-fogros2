
from launch import LaunchDescription
from launch_ros.actions import Node


def generate_launch_description():
    """Talker example that launches everything locally."""
    ld = LaunchDescription()

    # talker_node = Node(
    #     package="bench", executable="listener",
    # )

    # ld.add_action(talker_node)

    client_node = Node(
        package="bench", executable="parking_publisher",
    )

    ld.add_action(client_node)

    sgc_router = Node(
        package="sgc_launch",
        executable="sgc_router", 
        output="screen",
        emulate_tty = True,
        parameters = [
            # find and add config file in ./sgc_launhc/configs
            # or use the `config_file_name` optional parameter
            {"config_file_name": "parking.yaml"}, 
            {"whoami": "machine_talker"},
            {"release_mode": True}
        ]
    )
    ld.add_action(sgc_router)
    
    return ld
