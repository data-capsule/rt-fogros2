import json
import glob
import matplotlib.pyplot as plt
import seaborn as sns
import argparse

def load_benchmark_log(filename):
    with open(filename, 'r') as f:
        return json.load(f)

def plot_benchmark_logs(log_files, labels=None):
    # Set the seaborn style
    sns.set_context("talk")
    plt.figure(figsize=(10, 6))
    
    if labels is None:
        labels = [f'{log_file.split("/")[-1]}' for log_file in log_files]
    
    for log_file, label in zip(log_files, labels):
        data = load_benchmark_log(log_file)
        print(data['total_latencies'])
        
        # Plot total average with seaborn
        sns.lineplot(x=data['message_sizes'], 
                    y=data['total_latencies'],
                    label=f'{label.strip(".json")}',
                    marker='o')
    
    plt.xscale('log')
    plt.yscale('log')
    plt.xlabel('Message Size (bytes)')
    plt.ylabel('Latency (ms)')
    # plt.title('Benchmark Results: Latency vs Message Size')
    
    # Adjust layout and save
    plt.tight_layout()
    plt.grid(True)
    plt.savefig('benchmark_comparison.png', bbox_inches='tight', dpi=300)
    print('Plot saved as benchmark_comparison.png')

def main():
    parser = argparse.ArgumentParser(description='Plot benchmark log files')
    parser.add_argument('log_files', nargs='+', help='Benchmark log files to plot')
    parser.add_argument('--labels', nargs='+', help='Labels for each benchmark')
    
    args = parser.parse_args()
    
    plot_benchmark_logs(args.log_files, args.labels)

if __name__ == '__main__':
    main() 