import rclpy
from rclpy.node import Node
from sensor_msgs.msg import CompressedImage
import numpy as np
import time
import cv2
import matplotlib.pyplot as plt

class BenchmarkPublisher(Node):
    def __init__(self):
        super().__init__('benchmark_publisher')
        
        # Benchmark parameters
        self.message_sizes = [64, 640, 6400, 64000, 640000]  # 64, 640, 6400, 64000, 640000, 6400000
        self.current_size_index = 0
        self.num_samples = 500
        self.current_sample = 0
        self.latency_sums = [0.0] * 4  # Sum for each stream
        
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
        self.timer = self.create_timer(0.066, self.timer_callback)  # 15Hz
        
        # Generate initial dummy data
        self.generate_dummy_data()
        
        # Add storage for plotting data
        self.plot_data = {
            'message_sizes': [],
            'stream_latencies': [[] for _ in range(4)],
            'total_latencies': []
        }
        
    def generate_dummy_data(self):
        current_size = self.message_sizes[self.current_size_index]
        self.dummy_bytes = np.random.randint(0, 254, current_size)
        self.get_logger().info(f'Generated new data size: {current_size} bytes')
        
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
        
        # Add to sums
        for i, latency in enumerate(latencies):
            self.latency_sums[i] += latency
            
        self.current_sample += 1
        
        # Check if we've collected enough samples
        if self.current_sample >= self.num_samples:
            # Calculate and log averages
            averages = [sum_lat / self.num_samples for sum_lat in self.latency_sums]
            total_avg = sum(averages)
            
            # Store data for plotting
            current_size = self.message_sizes[self.current_size_index]
            self.plot_data['message_sizes'].append(current_size)
            for i, avg in enumerate(averages):
                self.plot_data['stream_latencies'][i].append(avg)
            self.plot_data['total_latencies'].append(total_avg)
            
            self.get_logger().info(f'=== Benchmark Results for {self.message_sizes[self.current_size_index]} bytes ===')
            for i, avg in enumerate(averages):
                self.get_logger().info(f'Stream {i} average latency: {avg:.3f} ms')
            self.get_logger().info(f'Total average latency: {total_avg:.3f} ms')
            
            # Reset for next size
            self.current_sample = 0
            self.latency_sums = [0.0] * 4
            
            # Move to next message size
            self.current_size_index += 1
            if self.current_size_index < len(self.message_sizes):
                self.generate_dummy_data()
            else:
                self.plot_results()
                self.get_logger().info('Benchmark complete!')
                rclpy.shutdown()

    def plot_results(self):
        # Save data to log file
        log_data = {
            'message_sizes': self.plot_data['message_sizes'],
            'stream_latencies': self.plot_data['stream_latencies'],
            'total_latencies': self.plot_data['total_latencies'],
            'timestamp': time.strftime("%Y%m%d-%H%M%S")
        }
        
        import json
        log_filename = f'./rt-fogros2/logs/benchmark_log_{log_data["timestamp"]}.json'
        with open(log_filename, 'w') as f:
            json.dump(log_data, f)
        self.get_logger().info(f'Log data saved as {log_filename}')

def main(args=None):
    rclpy.init(args=args)
    publisher = BenchmarkPublisher()
    rclpy.spin(publisher)
    publisher.destroy_node()
    rclpy.shutdown()

if __name__ == '__main__':
    main() 