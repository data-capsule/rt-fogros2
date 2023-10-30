
from .web_requests import *
import yaml 
import pprint

class Node:
    def __init__(self, address, parent=None):
        self.address = address
        self.children = []
        self.parent = parent

def build_mcast_tree(node_dict, parent=None):
    address = node_dict.get("address")
    node = Node(address, parent)
    
    for child_dict in node_dict.get("children", []):
        child_node = build_mcast_tree(child_dict, node)
        node.children.append(child_node)
    
    return node

def build_tree(node_dict, parent=None):
    if isinstance(node_dict, str):
        return Node(node_dict, parent)
    # logger.info(f"node dict {node_dict}")
    address = list(node_dict.keys())[0] # only one key, because its a tree
    node = Node(address, parent)
    
    # logger.info(f"{node_dict[address]['children']}")
    for child_dict in node_dict[address]['children']:
        child_node = build_tree(child_dict, node)
        node.children.append(child_node)
    
    return node

def print_tree(node, level=0):
    print("  " * level + f"{node.address}")
    for child in node.children:
        print_tree(child, level + 1)


class SGC_StateMachine: 
    def __init__(self, state_name, topic_dict, param_dict, service_dict):
        self.state_name = state_name
        self.topics = topic_dict
        self.params = param_dict
        self.services = service_dict

    def __repr__(self):
        return str(self.__dict__)
    
class SGC_Swarm: 
    def __init__(self, yaml_config, 
                 whoami, logger,
                 sgc_address):
        # the identifers for the task and ROS instance
        self.task_identifier = None 
        self.instance_identifer = whoami 
        
        # default Berkeley's parameters 
        self.signaling_server_address = 'ws://3.18.194.127:8000'
        self.routing_information_base_address = '3.18.194.127:8002'
        self.sgc_address = sgc_address

        # topic dictionary: map topic to topic type 
        self.topic_dict = dict()
        self.service_dict = dict()

        # states: map state_name to SGC_StateMachine
        self.state_dict = dict()

        # assignment: map identifer to state_names 
        self.assignment_dict = dict()

        self._paused_topics = []
        self._paused_services = []

        self.logger = logger
        logger = self.logger
        self.config = None
        
        self.load(yaml_config)

    def load(self, yaml_config):
        with open(yaml_config, "r") as f:
            config = yaml.safe_load(f)
            self.config = config 
            self.logger.info(f"The config file is \n {pprint.pformat(config)}")
        self._load_addresses(config)
        self._load_identifiers(config)
        self._load_services(config)
        self._load_topics(config)
        # print_tree(build_tree(config["topology"]))
        self._load_state_machine(config)
        
    '''
    apply assignment dictionary
    '''
    def apply_assignment(self, new_assignment_dict):
        if self.instance_identifer not in new_assignment_dict:
            self.logger.warn(f"[Warn] the assignment dict {new_assignment_dict} doesn't have the identifier {self.instance_identifer} for this machine")
        
        for machine in new_assignment_dict:
            if machine == self.instance_identifer:
                # conduct actual parameter change if the identifier is in the assignment dict
                previous_state = self.assignment_dict[self.instance_identifer] if self.instance_identifer in self.assignment_dict else None 
                current_state = new_assignment_dict[self.instance_identifer]
                if self.instance_identifer in self.assignment_dict and \
                    previous_state != current_state:
                    self.logger.warn("the assignment has changed! need to revoke the current assignment ")
                    for topic_to_action_pair in self.state_dict[previous_state].topics:
                        topic_name = list(topic_to_action_pair.keys())[0] # because it only has one element for sure
                        topic_type = self.topic_dict[topic_name]
                        topic_action = topic_to_action_pair[topic_name]
                        send_topic_request("del", topic_action, topic_name, topic_type, self.sgc_address)
                        self._paused_topics.append(topic_name)
                            
                # add in new topics 
                if self.state_dict[current_state].topics:
                    for topic_to_action_pair in self.state_dict[current_state].topics:
                        topic_name = list(topic_to_action_pair.keys())[0] # because it only has one element for sure
                        topic_type = self.topic_dict[topic_name]
                        topic_action = topic_to_action_pair[topic_name]
                        if topic_name in self._paused_topics:
                            # if the topic is paused, we need to resume it 
                            self.logger.warn(f"resuming topic {topic_name} this prevents setting up a new connection")
                            send_topic_request("resume", topic_action, topic_name, topic_type, self.sgc_address)
                            self._paused_topics.remove(topic_name)
                        else:
                            self.logger.warn(f"adding topic {topic_name} to SGC router")
                            send_topic_request("add", topic_action, topic_name, topic_type, self.sgc_address)
                            #self.construct_tree_by_sending_request_topic(build_tree(self.config["topology"]), topic_name, topic_type)

                # add in new services 
                if self.state_dict[current_state].services:
                    for service_to_action_pair in self.state_dict[current_state].services:
                        service_name = list(service_to_action_pair.keys())[0] # because it only has one element for sure
                        service_type = self.service_dict[service_name]
                        service_action = service_to_action_pair[service_name]
                        if service_name in self._paused_services:
                            # if the topic is paused, we need to resume it 
                            self.logger.warn(f"resuming service {service_name} this prevents setting up a new connection")
                            send_service_request("resume", service_action, service_name, service_type, self.sgc_address)
                            self._paused_services.remove(service_name)
                        else:
                            self.logger.warn(f"adding service {service_name} to SGC router")
                            send_service_request("add", service_action, service_name, service_type, self.sgc_address)
                            # self.construct_tree_by_sending_request_service(build_tree(self.config["topology"]), topic_name, topic_type)

                self.assignment_dict[machine] = new_assignment_dict[machine]
            else:
                # only udpate the assignment dict, do not do any parameter change
                self.assignment_dict[machine] = new_assignment_dict[machine]

        # TODO: apply parameter changes 

    def _load_addresses(self, config):
        if "addresses" not in config:
            return 
        self.signaling_server_address = config["addresses"]["signaling_server_address"] if "signaling_server_address" in config["addresses"] else self.signaling_server_address
        self.routing_information_base_address = config["addresses"]["routing_information_base_address"] if "routing_information_base_address" in config["addresses"] else self.routing_information_base_address

    def _load_identifiers(self, config):
        self.task_identifier = config["identifiers"]["task"]
        if "whoami" not in config["identifiers"]: 
            # whoami not defined in the rosparam, we directly use the value from config file
            # the value is already in self.instance_identifier
            if not self.instance_identifer:
                self.logger.error("Both rosparam and config file do not define whoami, define it")
                exit()
        else:
            # either way,if rosparam is already defined, use rosparam's value
            if self.instance_identifer:
                self.logger.warn("both ros param and config file defines whoami, using the value from rosparam")
            else:
                self.instance_identifer = config["identifiers"]["whoami"]

    def _load_topics(self, config):
        if "topics" not in config:
            return
        for topic in config["topics"]:
            self.topic_dict[topic["topic_name"]] = topic["topic_type"]

    def _load_services(self, config):
        for service in config["services"]:
            self.service_dict[service["service_name"]] = service["service_type"]

    def _load_state_machine(self, config):
        for state_name in config["state_machine"]:
            state_description = config["state_machine"][state_name]
            if state_description:
                topics =  state_description["topics"] if "topics" in state_description else None 
                params =  state_description["params"] if "params" in state_description else None 
                services = state_description["services"] if "services" in state_description else None
                self.state_dict[state_name] = SGC_StateMachine(state_name, topics, params, services)
            else:
                self.state_dict[state_name] = SGC_StateMachine(state_name, None, None, None)
        self.logger.info(str(self.state_dict))

    def get_assignment_from_yaml(self, yaml_path):
        with open(yaml_path, "r") as f:
            config = yaml.safe_load(f)
        for identity_name in config["assignment"]:
            state_name = config["assignment"][identity_name]
            if state_name not in self.state_dict:
                self.logger.warn(f"State {state_name} not defined. Not added!")
                continue 
            self.assignment_dict[identity_name] = state_name
        return self.assignment_dict
    
    def construct_tree_by_sending_request_service(self, node, topic_name, topic_type):
        for child in node.children:
            # uniquely identify the session
            session_id = node.address + child.address

            if node.address == self.instance_identifer:
                # establish request channel from node to child
                send_routing_request_service(
                    self.sgc_address,
                    topic_name, 
                    topic_type,
                    "source",
                    "request" + node.address + session_id,
                    "request" + child.address + session_id,
                    "request"
                )

                send_routing_request_service(
                    self.sgc_address,
                    topic_name, 
                    topic_type,
                    "destination",
                    "response" + child.address + session_id,
                    "response" + node.address + session_id,
                    "response"
                )

            if child.address == self.instance_identifer:
                # establish response channel from child to node
                send_routing_request_service(
                    self.sgc_address,
                    topic_name, 
                    topic_type,
                    "destination",
                    "request" + node.address + session_id,
                    "request" + child.address + session_id,
                    "request"
                )
                
                send_routing_request_service(
                    self.sgc_address,
                    topic_name, 
                    topic_type,
                    "source",
                    "response" + child.address + session_id,
                    "response" + node.address + session_id,
                    "response"
                )

            self.construct_tree_by_sending_request_service(child, topic_name, topic_type)


    def construct_tree_by_sending_request_topic(self, node, topic_name, topic_type):
        for child in node.children:
            # uniquely identify the session
            session_id = node.address + child.address

            if node.address == self.instance_identifer:
                # establish request channel from node to child
                send_routing_request_topic(
                    self.sgc_address,
                    topic_name, 
                    topic_type,
                    "source",
                    "topic" + node.address + session_id,
                    "topic" + child.address + session_id,
                    "pub"
                )
            if child.address == self.instance_identifer:
                send_routing_request_topic(
                self.sgc_address,
                "destination",
                "topic" + node.address + session_id,
                "topic" + child.address + session_id,
                "pub"
                )

            self.construct_tree_by_sending_request_topic(child, topic_name, topic_type)

    # phase 1: only allow changing the state on state machine 
    # TODO: allowing changing the state machine (not a must)
    def update():
        pass 
