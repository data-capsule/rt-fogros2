import rclpy
from rclpy.node import Node
from sensor_msgs.msg import CompressedImage
import numpy as np
import time
import cv2

class BenchmarkPublisher(Node):
    def __init__(self):
        super().__init__('benchmark_publisher')
        
        # Create 4 publishers for compressed image streams
        self.pubs = []
        for i in range(4):
            pub = self.create_publisher(CompressedImage, f'parking_stream_{i}/compressed', 10)
            self.pubs.append(pub)
            
        # Create subscriber for latency feedback
        self.latency_sub = self.create_subscription(
            CompressedImage,
            'latency_feedback/compressed',
            self.latency_callback,
            10)
            
        # Create timer for publishing
        self.timer = self.create_timer(0.1, self.timer_callback)  # 10Hz
        
        # Generate dummy image data instead of random bytes
        self.dummy_bytes = np.random.randint(0, 254, 64) #64k bytes 
        
    def timer_callback(self):
        msg = CompressedImage()
        msg.format = "bytes"
        
        msg.data = self.dummy_bytes.tobytes()
        
        # Publish to all streams
        for pub in self.pubs:
            pub.publish(msg)
            
    def latency_callback(self, msg):
        # Deserialize latencies from bytes
        latencies = np.frombuffer(msg.data, dtype=np.float32)
        for i, latency in enumerate(latencies):
            self.get_logger().info(f'Stream {i} latency: {latency:.3f} ms')

def main(args=None):
    rclpy.init(args=args)
    publisher = BenchmarkPublisher()
    rclpy.spin(publisher)
    publisher.destroy_node()
    rclpy.shutdown()

if __name__ == '__main__':
    main() 