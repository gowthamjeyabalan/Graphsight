#!/usr/bin/env python3
"""Generate a sample dataset: a stochastic-block-model social graph with node attributes."""
import csv, random, sys
random.seed(7)
N = int(sys.argv[1]) if len(sys.argv) > 1 else 1200
groups = ["Engineering", "Design", "Sales", "Support", "Research"]
countries = ["India", "USA", "Germany", "Brazil", "Japan", "Kenya", "UK", "Canada"]
roles = ["junior", "mid", "senior", "lead"]
sizes = [0.30, 0.15, 0.25, 0.18, 0.12]
node_group = [random.choices(range(5), sizes)[0] for _ in range(N)]
with open("nodes.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["id", "name", "group", "role", "country", "age", "score", "tenure_years", "remote"])
    for i in range(N):
        g = node_group[i]
        age = int(random.gauss(34 + g * 2, 8))
        age = max(19, min(65, age))
        score = round(random.lognormvariate(1.2 + g * 0.15, 0.7), 2)
        w.writerow([f"n{i}", f"Person {i}", groups[g], random.choices(roles, [4, 3, 2, 1])[0],
                    random.choice(countries), age, score, round(max(0, random.gauss(4, 3)), 1),
                    random.choice(["true", "false"])])
edges = set()
p_in, p_out = 6.0 / N * 5, 0.35 / N
by_group = [[i for i in range(N) if node_group[i] == g] for g in range(5)]
for g in range(5):
    m = by_group[g]
    for _ in range(int(len(m) * 3.2)):
        a, b = random.sample(m, 2)
        edges.add((min(a, b), max(a, b)))
# a few hubs
for h in random.sample(range(N), 12):
    for _ in range(random.randint(15, 40)):
        t = random.randrange(N)
        if t != h:
            edges.add((min(h, t), max(h, t)))
for _ in range(int(N * 0.35)):
    a, b = random.sample(range(N), 2)
    edges.add((min(a, b), max(a, b)))
kinds = ["friend", "colleague", "mentor", "family"]
with open("edges.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["source", "target", "weight", "kind"])
    for a, b in sorted(edges):
        w.writerow([f"n{a}", f"n{b}", random.randint(1, 10), random.choices(kinds, [5, 6, 2, 1])[0]])
print(N, "nodes", len(edges), "edges")
