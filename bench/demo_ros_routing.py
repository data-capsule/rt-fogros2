import requests 
from time import sleep
import yaml
import hashlib
switch = "localhost:3003"
server = "localhost:3002"
client = "localhost:3005"


# server = "localhost:3002"
# ros_topic = {
#     "api_op": "routing",
#     "ros_op": "destination",
#     "crypto": "test_cert",
#     "topic_name": "/add_three_ints",
#     "topic_type": "bench_msgs/srv/AddThreeInts",
#     "connection_type": "request",
#     "forward_sender_url": "sender-source-to-dst", 
#     "forward_receiver_url": "receiver-source-to-dst"
# }
# uri = f"http://{server}/service"
# # Create a new resource
# response = requests.post(uri, json = ros_topic)
# print(response)

def send_routing_request(addr, source_or_destination, sender_url, receiver_url, connection_type):
    sha = hashlib.sha256()
    sha.update(sender_url.encode())
    sender_url = sha.hexdigest()
    sha = hashlib.sha256()
    sha.update(receiver_url.encode())
    receiver_url = sha.hexdigest()
    ros_topic = {
        "api_op": "routing",
        "ros_op": source_or_destination,
        "crypto": "test_cert",
        "topic_name": "/add_three_ints",
        "topic_type": "bench_msgs/srv/AddThreeInts",
        "connection_type": connection_type,
        "forward_sender_url": sender_url, 
        "forward_receiver_url": receiver_url
    }
    print(addr, ros_topic)
    uri = f"http://{addr}/service"
    # Create a new resource
    response = requests.post(uri, json = ros_topic)
    print(response)


class Node:
    def __init__(self, address, is_compute=False, parent=None):
        self.address = address
        self.is_compute = is_compute
        self.children = []
        self.parent = parent

def build_tree(node_dict, parent=None):
    address = node_dict.get("address")
    is_compute = node_dict.get("is_compute", False)
    node = Node(address, is_compute, parent)
    
    for child_dict in node_dict.get("children", []):
        child_node = build_tree(child_dict, node)
        node.children.append(child_node)
    
    return node

def print_tree(node, level=0):
    print("  " * level + f"{node.address}, is_compute: {node.is_compute}")
    for child in node.children:
        print_tree(child, level + 1)


def construct_tree_by_sending_request(node):
    for child in node.children:
        send_routing_request(
            node.address,
            "source",
            "request" + node.address,
            "request" + child.address,
            "request"
        )
        send_routing_request(
            child.address,
            "destination",
            "request" + node.address,
            "request" + child.address,
            "request"
        )
        send_routing_request(
            child.address,
            "source",
            "response" + child.address,
            "response" + node.address,
            "response"
        )
        send_routing_request(
            node.address,
            "destination",
            "response" + child.address,
            "response" + node.address,
            "response"
        )
        construct_tree_by_sending_request(child)

# Your YAML string
# yaml_str = '''
# root:
#   address: "client"
#   is_compute: true
#   children:
#     - address: "switch"
#       is_compute: false
#       children:
#         - address: "server"
#           is_compute: true
# '''

yaml_str = f'''
root:
  address: {client}
  is_compute: true
  children:
    - address: {server}
      is_compute: true
'''

# Load YAML into a Python dictionary
yaml_dict = yaml.safe_load(yaml_str)

# Build the tree
root_dict = yaml_dict['root']
root_node = build_tree(root_dict)
print_tree(root_node)
construct_tree_by_sending_request(root_node)

# robot_topics = reverse_topics(service_topics)

# cloud = Machine("fogros2-sgc-lite-listener-1:3000")
# robot = Machine("localhost:3000")

# add_topics_to_machine(robot_topics, robot)
# input()
# remove_topics_from_machine(service_topics, cloud)
# remove_topics_from_machine(robot_topics, robot)

# talker_machine = ""
# listener_machine = ""

# print("adding listener")
# send_request("add", "pub", ip = listener_machine)

# print("adding talker")
# send_request("add", "sub", ip = talker_machine)

# sleep(5)

# print("remove the topic of talker's published topic")
# send_request("del", "sub", ip = talker_machine)

# sleep(5)
# print("adding it back")
# send_request("add", "sub", ip = talker_machine)


# for i in range(10):
#     sleep(1)
#     print(".")

# print("remove the topic of Machine 1's published topic")
# send_request("del", "sub", ip="172.190.80.56")
# print("have machine 2 (Another AWs) to publish the topic")
# send_request("add", "sub", ip="54.67.119.252")
# print("locally publish chatter topic")
# send_request("add", "pub")
# migration!
# print("remove the topic of Machine 1's published topic")
# send_request("del", "sub", ip="172.190.80.56")
# print("have machine 2 (Another AWs) to publish the topic")
# send_request("add", "sub", ip="54.67.119.252")


# available actions 
# pub enum FibChangeAction {
#     ADD,
#     PAUSE, // pausing the forwarding of the topic, keeping connections alive
#     PAUSEADD, // adding the entry to FIB, but keeps it paused
#     RESUME, // resume a paused topic
#     DELETE, // deleting a local topic interface and all its network connections 
# }

# class Topic: 
#     def __init__(self, name, type, action):
#         self.name = name
#         self.type = type 
#         self.action = action

# class Machine:
#     def __init__(self, address):
#         self.address = address

# def reverse_topics(topic_list):
#     ret = []
#     for topic in topic_list:
#         if topic.action == "sub":
#             action = "pub"
#         if topic.action == "pub":
#             action = "sub"
#         if topic.action == "noop":
#             action = "noop"
#         ret.append(Topic(
#             topic.name,
#             topic.type,
#             action
#         ))
#     return ret

# def send_request(
#     api_op, 
#     topic,
#     machine, 
# ):
#     ros_topic = {
#         "api_op": api_op,
#         "ros_op": topic.action,
#         "crypto": "test_cert",
#         "topic_name": topic.name,
#         "topic_type": topic.type,
#     }
#     uri = f"http://{machine.address}/service"
#     # Create a new resource
#     response = requests.post(uri, json = ros_topic)
#     print(response)

# def add_topics_to_machine(topics, machine):
#     for topic in topics:
#         send_request("add", topic, machine)
#         # sleep(2)

# def remove_topics_from_machine(topics, machine):
#     for topic in topics:
#         send_request("del", topic, machine)
#         # sleep(2)
        
# robot_service = [
#     Topic(
#     "/add_three_ints", "bench_msgs/srv/AddThreeInts", "client"
# )]

# server_service = [
#     Topic(
#     "/add_three_ints", "bench_msgs/srv/AddThreeInts", "service"
# )]



# client = "localhost:3005"
# ros_topic = {
#     "api_op": "routing",
#     "ros_op": "source",
#     "crypto": "test_cert",
#     "topic_name": "/add_three_ints",
#     "topic_type": "bench_msgs/srv/AddThreeInts",
#     "connection_type": "request",
#     "forward_sender_url": "sender-source-to-dst", 
#     "forward_receiver_url": "receiver-source-to-dst"
# }
# uri = f"http://{client}/service"
# # Create a new resource
# response = requests.post(uri, json = ros_topic)
# print(response)


# server = "localhost:3002"
# ros_topic = {
#     "api_op": "routing",
#     "ros_op": "destination",
#     "crypto": "test_cert",
#     "topic_name": "/add_three_ints",
#     "topic_type": "bench_msgs/srv/AddThreeInts",
#     "connection_type": "request",
#     "forward_sender_url": "sender-source-to-dst", 
#     "forward_receiver_url": "receiver-source-to-dst"
# }
# uri = f"http://{server}/service"
# # Create a new resource
# response = requests.post(uri, json = ros_topic)
# print(response)


# ros_topic = {
#     "api_op": "routing",
#     "ros_op": "source",
#     "crypto": "test_cert",
#     "topic_name": "/add_three_ints",
#     "topic_type": "bench_msgs/srv/AddThreeInts",
#     "connection_type": "response",
#     "forward_sender_url": "sender-dst-to-source", 
#     "forward_receiver_url": "receiver-dst-to-source", 
# }
# uri = f"http://{server}/service"
# response = requests.post(uri, json = ros_topic)
# print(response)


# addr = "localhost:3005"
# ros_topic = {
#     "api_op": "routing",
#     "ros_op": "destination",
#     "crypto": "test_cert",
#     "topic_name": "/add_three_ints",
#     "topic_type": "bench_msgs/srv/AddThreeInts",
#     "connection_type": "response",
#     "forward_sender_url": "sender-dst-to-source", 
#     "forward_receiver_url": "receiver-dst-to-source", 
# }
# uri = f"http://{client}/service"
# response = requests.post(uri, json = ros_topic)
# print(response)
