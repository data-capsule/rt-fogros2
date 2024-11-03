import rclpy
from rclpy.node import Node
from sensor_msgs.msg import CompressedImage
import time
import numpy as np
import cv2

class BenchmarkSubscriber(Node):
    def __init__(self):
        super().__init__('benchmark_subscriber')
        
        # Create 4 subscribers for compressed image streams
        self.subscribers = []
        self.latencies = {i: 0.0 for i in range(4)}
        
        for i in range(4):
            sub = self.create_subscription(
                CompressedImage,
                f'parking_stream_{i}/compressed',
                lambda msg, stream_id=i: self.callback(msg, stream_id),
                10)
            self.subscribers.append(sub)
            
        # Create publisher for latency feedback
        self.latency_pub = self.create_publisher(
            CompressedImage,
            'latency_feedback/compressed',
            10)
            
        # Timer for publishing latency statistics
        self.timer = self.create_timer(0.066, self.publish_latencies)  # 1Hz
        
    def callback(self, msg, stream_id):
        self.get_logger().info(f'Received message from stream {stream_id}')
        self.latencies[stream_id] = time.time()
        
    def publish_latencies(self):
        # Calculate average latencies
        avg_latencies = []
        for i in range(4):
            if self.latencies[i]:
                avg_latency = time.time() - self.latencies[i]
                avg_latencies.append(avg_latency)
            else:
                avg_latencies.append(0.0)
                
        # Convert to numpy array and serialize to bytes
        latency_array = np.array(avg_latencies, dtype=np.float32)
        
        # Create and publish compressed message
        msg = CompressedImage()
        msg.format = "bytes"  # Indicating raw bytes, not an image
        msg.data = latency_array.tobytes()
        self.latency_pub.publish(msg)

def main(args=None):
    rclpy.init(args=args)
    subscriber = BenchmarkSubscriber()
    rclpy.spin(subscriber)
    subscriber.destroy_node()
    rclpy.shutdown()

if __name__ == '__main__':
    main() 