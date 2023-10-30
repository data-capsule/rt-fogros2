

import hashlib
import requests
from time import sleep 

def send_routing_request_service(addr,topic_name, topic_type, source_or_destination, sender_url, receiver_url, connection_type):
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
        "topic_name": topic_name,
        "topic_type": topic_type,
        "connection_type": connection_type,
        "forward_sender_url": sender_url, 
        "forward_receiver_url": receiver_url
    }
    print(addr, ros_topic)
    uri = f"http://{addr}/service"
    # Create a new resource
    response = requests.post(uri, json = ros_topic)
    print(response)
    sleep(1)


def send_routing_request_topic(addr, topic_name, topic_type, source_or_destination, sender_url, receiver_url, connection_type):
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
        "topic_name": topic_name,
        "topic_type": topic_type,
        "connection_type": connection_type,
        "forward_sender_url": sender_url, 
        "forward_receiver_url": receiver_url
    }
    print(addr, ros_topic)
    uri = f"http://{addr}/topic"
    # Create a new resource
    response = requests.post(uri, json = ros_topic)
    print(response)
    sleep(1)


def send_topic_request(
    api_op, 
    topic_action,
    topic_name,
    topic_type,
    machine_address, 
):
    ros_topic = {
        "api_op": api_op,
        "ros_op": topic_action,
        "crypto": "test_cert",
        "topic_name": topic_name,
        "topic_type": topic_type,
    }
    uri = f"http://{machine_address}/topic"
    # Create a new resource
    response = requests.post(uri, json = ros_topic)
    # print(f"topic {topic.name} with operation {api_op} request sent with response {response}")


def send_service_request(
    api_op, 
    topic_action,
    topic_name,
    topic_type,
    machine_address, 
):
    ros_topic = {
        "api_op": api_op,
        "ros_op": topic_action,
        "crypto": "test_cert",
        "topic_name": topic_name,
        "topic_type": topic_type,
    }
    uri = f"http://{machine_address}/service"
    # Create a new resource
    response = requests.post(uri, json = ros_topic)
    # print(f"topic {topic.name} with operation {api_op} request sent with response {response}")


