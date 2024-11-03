import json
import glob
import matplotlib.pyplot as plt
import argparse

def load_benchmark_log(filename):
    with open(filename, 'r') as f:
        return json.load(f)

def plot_benchmark_logs(log_files, labels=None):
    plt.figure(figsize=(12, 6))
    
    if labels is None:
        labels = [f'Benchmark {i+1}' for i in range(len(log_files))]
    
    for log_file, label in zip(log_files, labels):
        data = load_benchmark_log(log_file)
        
        # Plot individual stream latencies
        linestyles = ['-', '--', ':', '-.']
        for i in range(4):
            plt.plot(data['message_sizes'], 
                    data['stream_latencies'][i], 
                    marker='o',
                    linestyle=linestyles[i],
                    label=f'{label} - Stream {i}')
        
        # Plot total average
        plt.plot(data['message_sizes'], 
                data['total_latencies'], 
                'k--', 
                linewidth=2, 
                label=f'{label} - Total Average')
    
    plt.xscale('log')
    plt.yscale('log')
    plt.xlabel('Message Size (bytes)')
    plt.ylabel('Latency (ms)')
    plt.title('Benchmark Results: Latency vs Message Size')
    plt.grid(True)
    plt.legend(bbox_to_anchor=(1.05, 1), loc='upper left')
    
    # Adjust layout to prevent legend cutoff
    plt.tight_layout()
    
    # Save the plot
    plt.savefig('benchmark_comparison.png', bbox_inches='tight')
    print('Plot saved as benchmark_comparison.png')

def main():
    parser = argparse.ArgumentParser(description='Plot benchmark log files')
    parser.add_argument('log_files', nargs='+', help='Benchmark log files to plot')
    parser.add_argument('--labels', nargs='+', help='Labels for each benchmark')
    
    args = parser.parse_args()
    
    plot_benchmark_logs(args.log_files, args.labels)

if __name__ == '__main__':
    main() 