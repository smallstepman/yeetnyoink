## Contents
- keywords:
- 1 Introduction
  - 1.1 RDG application
  - 1.2 Previous Study
  - 1.3 Gap in the existing work
    - Theorem 1.1 .
    - Theorem 1.2 .
  - 1.4 Results
- 2 Preliminaries
  - Definition 2.1 .
  - Definition 2.2 .
  - Definition 2.3 .
  - Definition 2.4 .
  - Theorem 2.1 .
  - Theorem 2.2 .
- 3 Extended RDG Construction
- 4 RDG Stereographic Projection
- 5 RDG Existence Theory
  - Theorem 5.1 .
  - Proof.
  - Theorem 5.2 .
  - Proof.
  - Theorem 5.3 .
  - Proof.
- 6 Concluding remarks and future task
- References

## Abstract

Abstract A plane graph is called a rectangular graph if each of its edges can be oriented either horizontally or vertically, each of its interior regions is a four-sided region and all interior regions can be fitted in a rectangular enclosure. Only planar graphs can be dualized. If the dual of a plane graph is a rectangular graph , then the plane graph is a rectangularly dualizable graph . In 1985, Koźmiński and Kinnen presented a necessary and sufficient condition for the existence of a rectangularly dualizable graph for a separable connected plane graph.
In this paper, we present a counter example for which the conditions given by them for separable connected plane graphs fail and hence,
we derive a necessary and sufficient condition for a plane graph to be a rectangularly dualizable graph .

###### keywords:

## 1 Introduction

The theory of rectangularly dualizable graphs plays an important role in floorplanning, particularly at large scale such as VLSI circuit design. It provides us information at early stage to decide whether a given plane graph can be realized by a rectangular floorplan (RFP). There exists a geometric duality relationship between plane graphs and rectangular floorplans (RFPs) which can be described as follows:

An RFP is a partition of a rectangle $\mathcal{R}$ into $n$ rectangles $R_{1},R_{2},\dots,R_{n}$ such that no four of them meet at a point. A graph $\mathcal{G}_{2}$ is a dual of a plane graph $\mathcal{G}_{1}$ if the vertices of $\mathcal{G}_{1}$ correspond to the regions of $\mathcal{G}_{1}$ and for every pair of adjacent vertices of $\mathcal{G}_{1}$, the corresponding regions in $\mathcal{G}_{2}$ are adjacent. A plane graph is called a rectangular graph if each of its edges can be oriented either horizontally or vertically, each of its interior regions is a four-sided region and all interior regions can be fitted in a rectangular enclosure. Only planar graphs can be dualized. If dual of a plane graph is a rectangular graph, then the plane graph is a rectangularly dualizable graphs (RDG). Thus an RFP can be seen as an embedding of the dual of a planar graph and it can formally be described as a rectangular dual graph of an RDG, i.e., for the dual of a RDG to be an RFP, we need to assign horizontal and vertical orientations to its edges. For a better clarification, consider a planar graph $\mathcal{G}$ shown in Fig. [1](#S1.F1)a. We form its extended graph (Fig. [1](#S1.F1)b.) by inserting cycle of length 4 at the exterior of $\mathcal{G}$ and then connecting the vertices of the cycle to the exterior vertices of $\mathcal{G}$. Then it is dualized in Fig. [1](#S1.F1)c. After assigning horizontal or vertical orientation to each of its edges, an embedding as shown in Fig. [1](#S1.F1)d. is obtained. In fact, it is an RFP. Thus $\mathcal{G}$ is rectangularly dualized to an RFP. This transformation is known as the rectangular dualization method which is well-studied in the literature.

Figure: Figure 1: Rectangular dualization: (a) plane graph, (b) extended plane graph, (c) rectangular dual graph and (d) rectangular floorplan
Refer to caption: /html/2102.05304/assets/x1.png

### 1.1 RDG application

A VLSI system structure is described by a graph where vertices correspond to component modules and edges correspond to required interconnections. For a given graph structure of a VLSI circuit, floorplanning is concerned with allocating space to component modules and their interconnections [2]. An embedding method given by Heller [2] enforces interconnection by abutment. Modules are designed in such a way that their connectors exactly match with their neighbors. Adapting this methodology, interconnections are coped with a clever design.

Due to the advancement of VLSI technology, it is extremely large. On the other hand, an RDG can handle atmost $3n-7$ interconnections [3], where $n$ is the number of modules. Consequent to this, its graph may not necessarily be planar and hence in modern VLSI system, component modules and interconnections can not be treated as independent entities. In such a situation, not all interconnections can be enforced by abutment. Linking the remaining interconnections with nonadjacent modules utilize additional routing space. For practicality of solution, a graph described by a VLSI circuit can be embedded in such a way that most of the interconnections can be made by abutment and the remaining interconnections linking with nonadjacent modules use additional routing space. For example, in Fig. [2](#S1.F2)d, $R_{7}$ and $R_{9}$, $R_{2}$ and $R_{6}$ are interconnected through shaded areas $R_{10}$ and $R_{11}$ respectively. These routing areas are anticipated by introducing crossover vertices(^3^33These vertices are introduced at the intersection of edges if it exists in order to embed a graph as a plane graph.) at a common point of intersection of edges.

The use of an RDG in floorplanning of a VLSI system can be illustrated by the following example. Consider a graph described by a VLSI system as shown in Fig. [2](#S1.F2)a. Note that input-output connections between VLSI system and outside world is represented by arrow heads. Although this graph is not planar, it is planarized by adding cross over vertices as shown in Fig. [2](#S1.F2)b. In order to satisfy the necessary adjacency requirements, new edges (red edges) have been added in Fig. [2](#S1.F2)c. After these modifications, it is possible to construct an RFP as shown in Fig. [2](#S1.F2)d where a component rectangle $R_{i}$ is dualized to a vertex $v_{i}$.

Figure: Figure 2: Constructing an RFP corresponding to a plane graph described by a VLSI system.
Refer to caption: /html/2102.05304/assets/x2.png

### 1.2 Previous Study

Investigations in the literature shows that the rectangular dualization theory [1, 4, 5, 6] of planar graphs is not much emphasized. It is known that every plane graph can be dualized, but not rectangularly dualized [1, 4, 5, 6]. Kozminski and Kinnen [1] derived a necessary and sufficient condition for a plane triangulated graph to be an RDG and implemented it in quadratic time [7]. Later, Bhasker and Sahni [4] improved this complexity to linear time implementing the rectangular dualization theory given by Kozminski and Kinnen [1]. Rinsma [5] showed through a counter example that it is not always possible for a vertex-weighted outer planar graph having 4 vertices of degree 2 to be an RDG. Besides this property, there are infinite outer planar graphs that are not rectangularly dualized. In fact, an outer planar graph having more than four critical shortcuts can not be rectangularly dualized. This can be contradicted by our proposed Theorem [5.2](#S5.Thmtheorem2) in Section [5](#S5). Since the graph structure of a VLSI system is never outer planar due to its large size, this theory can not be preferable for VLSI circuit’s design. The theory of rectangularly dualizable outer planar graphs plays a limited role in building architecture also. Lai and Leinward [6] showed that solving an RFP problem of a planar graph is equivalent to a matching problem of a bipartite graph derived from the given graph. This theory relies on the assigned regions to vertices of a graph. But this theory is not easy to implement, i.e., how can we check the assignments of regions to vertices in an arbitrary given plane graph? In fact, this theory is not implementable until a method for checking assignments of regions to vertices in an EPTG (extended planar triangulated graph) is known.

For practical use to VLSI field, many constructive algorithms [8, 9, 10, 11, 12, 13, 14, 15, 16, 17] based on the graph dualization theory were developed.

Counting of RFPs has been remained a great issue in combinatorics [18, 19, 20, 21, 22, 23, 24] because it produces a large solution space. To find a good condition solution in such large solution space is very hard and time consuming. Also, in these approaches, attention is given to blocks-packing in the minimal rectangular area. The other major concerns such as interconnection wire length is lagged behind.

With the renewed interest in floorplanning, floorplans are constructed using rectilinear modules (concave module) [25, 26, 27, 28, 29] also. Since a concave rectilinear module is made up of more than one rectangle, its design complexity is higher than a rectangular module (convex). Constructing a floorplan using concave rectilinear modules may decrease the quality of the floorplan.

### 1.3 Gap in the existing work

Motivated by the following points, we here find a necessary and sufficient condition for a given plane graph to be an RDG.

- i.
Kozminski and Kinnen [1] found the following necessary and sufficient for a separable connected graph to be an RDG.
Theorem 1.1.
[1, Theorem 3] Suppose that $\mathcal{G}$ is a separable connected plane graph with each of its interior faces triangular. $\mathcal{G}$ is an RDG if and only if
(a)
$\mathcal{G}$ has no separating triangle,
(b)
block neighborhood graph (BNG) is a path,
(c)
each maximal blocks(^4^44A maximal block of a graph $\mathcal{G}$ is a biconnected subgraph of $\mathcal{G}$ which is not contained in any other block.) corresponding to the endpoints of the BNG contains at most 2 critical corner implying paths,
(d)
no other maximal block contains a critical corner implying path.
The proof of this theorem is not rigor, but an outline. Furthermore, we present a counter example showing that it does not work properly for all separable connected graphs (see Fig. [3](#S1.F3)).
Figure 3: A counter example contradicting Theorem [1.1](#S1.Thmtheorem1).
Although the given graph $\mathcal{G}$ in Fig. [3](#S1.F3) satisfies all the conditions given in Theorem [1.1](#S1.Thmtheorem1), it is not an RDG. Using the existing algorithm [8], one can find an RFP for each of its blocks. Then, an RFP for $\mathcal{G}$ can be obtained by gluing them in a rectangular area which is not possible because of the adjacency relation of cut vertices $v_{4}$ and $v_{6}$. Corresponding to a cut vertex, there always associate a through rectangle(^5^55A through rectangle shares two sides to the exterior, but they are opposite sides.)[30] in an RFP for $\mathcal{G}$. But in Fig. [3](#S1.F3), the cut-vertices are adjacent. Hence, it is not possible to maintain rectangular enclosure while keeping $R_{4}$ and $R_{6}$ as through rectangles.
- ii.
Except Theorem [1.1](#S1.Thmtheorem1), there does not exist a theorem to check the existence of an RFP for separable connected planar graphs in the literature.
- iii.
Lai and Leinward [6] derived the following necessary and sufficient condition for an extended plane triangulated graph (EPTG) to be an RDG:
Theorem 1.2.
[6, Theorem 3] An EPTG is an RDG if and only if each of its triangular regions can be assigned to one of its corner vertices such that each vertex $v_{i}$ has exactly $d(v_{i})-4$ triangular region assigned to it.
This theory is not implementable until a method for checking assignments of regions to vertices in an EPTG is known.
- iv.
As discussed above Rinsma’s work [5] does not fully cover the class of all rectangularly dualizable outer planar graphs.

###### Theorem 1.1 .

###### Theorem 1.2 .

### 1.4 Results

In this paper, we find a necessary and sufficient condition for a given plane graph to be an RDG. We show that the unbounded region of an RFP can be realized by an unbounded rectangle in the extended Euclidean plane. Equivalently, we can say that an RFP can be seen as a quadrangulation of the Euclidean plane for which we find a stereographic projection of an RDG.

A brief description of our contribution is as follows:
In Section [2](#S2), we discuss existing facts about RDGs. Section [3](#S3) describes the extended RDG construction process. In Section [4](#S4), we find stenographic projection of the dual of an RDG in order to extract some result pertaining to the exterior (unbounded) region of the dual. In Section [5](#S5), we derive a necessary and sufficient condition for an EPTG to be an RDG. Finally, we conclude our contribution and discuss future scope in Section [6](#S6).

A list of notations used in this paper can be seen in Table 1.

**Table 1: List of Notations**
| Symbol | Description |
| --- | --- |
| RFP | rectangular floorplan |
| RDG | rectangularly dualizable graph |
| PTG | plane triangulated graph |
| EPTG | extended plane triangulated graph |
| $\mathcal{G}$ | a simple connected planar triangulated graph |
| $\mathcal{G}^{*}$ | Extended plane triangulated graph |
| $v_{i}$ | $i^{\text{th}}$ vertex of a graph |
| $d(v_{i})$ | degree of $v_{i}$ |
| $(v_{i},v_{j})$ | an edge incident to vertices $v_{i}$ and $v_{j}$ |
| $R_{i}$ | $i^{\text{th}}$ rectangle (region) of an RFP (RDG) corresponding to $v_{i}$ |

## 2 Preliminaries

In this section, we survey several facts about RFPs that would be helpful to prove our results.

A graph is called planar if it can be drawn in the Euclidean plane without crossing its edges except endpoints. A plane graph is a planar graph with a fixed planar drawing. It splits the Euclidean plane into connected regions called faces; the unbounded region is the exterior face (the outermost face) and all other faces are interior faces. The vertices lying on the exterior face are exterior vertices and all other vertices are interior vertices. A graph is said to be $k-$connected if it has at least $k$ vertices and the removal of fever than $k$ vertices does not disconnect the graph. If a connected graph has a cut vertex, then it is called a separable graph, otherwise it is called a nonseparable graph. Since floorplans are concerned with connectivity, we only consider nonseparable (biconnected) and separable connected graphs in this paper. A plane graph is called plane triangulated graph (PTG) if it has triangular faces. A TPG may or may not have exterior face triangular. In this paper, an PTG represents a plane graph with interior triangular faces.

###### Definition 2.1 .

A graph is said to be rectangular graph if each of its edges can be oriented horizontally or vertically such that it encloses a rectangular area. If the dual graph of a planar graph is a rectangular graph, then the graph is said to be a rectangularly dualizable graphs (RDG). In other words, a planar graph is rectangular dualizable (RDG) if its dual can be realized as a rectangular floorplan (RFP). An RFP is a partition of a rectangle $\mathcal{R}$ into $n$ rectangles $R_{1},R_{2},\dots,R_{n}$ provided that no four of them meet at a point.

###### Definition 2.2 .

[1] The block neighborhood graph (BNG) of a plane graph $\mathcal{G}$ is a graph where vertices are represented by biconnected components of $\mathcal{G}$ such that there is an edge between two vertices if and only if the two biconnected components they represent, have a vertex in common.

###### Definition 2.3 .

[1] A shortcut in a plane block $\mathcal{G}$ is an edge that is incident to two vertices on the outermost cycle $C$ of $\mathcal{G}$ and is not a part of $C$. A corner implying path (CIP) in $\mathcal{G}$ is a $v_{1}-v_{k}$ path on the outermost cycle of $\mathcal{G}$ such that it does not contain any vertices of a shortcut other than $v_{1}$ and $v_{k}$ and the shortcut $(v_{1},v_{k})$ is called a critical shortcut.

For a better understanding to Definition [2.3](#S2.Thmdefinition3), consider a graph shown in Fig. [4](#S2.F4). Edges $(v_{1},v_{3})$, $(v_{4},v_{9})$ and $(v_{6},v_{8})$ are shortcuts. Paths $v_{1}v_{2}v_{3}v_{4}$ and $v_{6}v_{7}v_{8}$ are CIPs while path $v_{9}v_{1}v_{2}v_{3}v_{4}$ is is not a CIP since it contains the endpoints of other shortcut $(v_{1},v_{3})$ and hence $(v_{9},v_{4})$ is not a critical shortcut. Both shortcuts $(v_{1},v_{3})$ and $(v_{6},v_{8})$ are of length 2.

Figure: Figure 4: (a) Presence of CIPs $v_{1}v_{2}v_{3}$ and (b) a separating triangle $v_{4}v_{6}v_{7}v_{4}$.
Refer to caption: /html/2102.05304/assets/x4.png

A component rectangle in an RFP is called a corner rectangle [30] if its two adjacent sides are adjacent to the exterior while a through rectangle shares its two opposite sides to the exterior. A component rectangle in an RFP is called an end rectangle if its three sides are adjacent to the exterior.

###### Definition 2.4 .

A separating cycle is a cycle in a plane graph $\mathcal{G}$ that encloses vertices inside as well as outside. If a separating cycle is of length 3, it is called a separating triangle or a complex triangle. We say a separating triangle in a plane graph is critical separating triangle if it does not contain any other separating triangle in its interior.

For instance, in Fig. [4](#S2.F4)b, the cycle $v_{4}v_{6}v_{7}v_{4}$ is a separating triangle while the cycle $v_{4}v_{8}v_{7}v_{4}$ is a critical separating triangle.

###### Theorem 2.1 .

[1, Theorem 3] A nonseperable plane graph $\mathcal{G}$ with triangular interior faces except exterior one is an RDG if and only if it has atmost 4 CIPs and has no separating triangle.

###### Theorem 2.2 .

[31] A graph $\mathcal{G}$ is 4-connected if and only if there exist atleast 4 vertex-disjoint path between any two vertices of $\mathcal{G}$.

###### Definition 2.1 .

###### Definition 2.2 .

###### Definition 2.3 .

###### Definition 2.4 .

###### Theorem 2.1 .

###### Theorem 2.2 .

## 3 Extended RDG Construction

In a graph described by a VLSI system, vertices and edges correspond to component modules and required interconnections respectively. Communication with units outside the given system are modeled by edges having one end incident to a vertex at the infinity (denoted by $v_{\infty}$, see Fig. [5](#S3.F5)). The vertex $v_{\infty}$ of an RDG corresponds to the unbounded region of its rectangular dual (RFP).

Only planar graphs can be dualized. Whenever the graph structure is not planar, it can be made planar by adding crossover vertices until the resultant graph is planar. Such vertices are inserted at crossings of edges in order to split the edges of a nonplanar graph. In general, maximum interconnections by abutment and minimum through channels is used as an objective function. Without loss of generality, we consider simple connected planar graphs in this paper.

In a floorplan, meeting $k$-component rectangles at a point is called $k$-joints. Since an RFP has three joints or four joints only, its dual has triangular or quadrangle regions only. Abiding by common design practice, we consider RFPs with three joints only. In fact, a quadrangle region can be partitioned into two triangular regions. In such cases, some extra adjacency requests allow unrelated components in the RDG to connect, but these connections are not used for interconnection.

Furthermore, a rectangular graph needs to be fitted in a rectangular enclosure while connecting to the outside world. Vertices that correspond to regions next to the enclosure are called enclosure vertices [9] and those vertices correspond to corner regions are called corner enclosure vertices. In figure [5](#S3.F5), vertices $v_{7},v_{6},v_{5},v_{4},v_{3},v_{2},v_{1}$ are enclosure vertices and $v_{7},v_{5},v_{3},v_{1}$ are corner enclosure vertices. Since the enclosure has 4 sides, out of these enclosure vertices, the enclosure corner vertices correspond to corner rectangles or end rectangles of an RFP where a corner rectangle shares its two sides to the unbounded (exterior) region and end rectangle shares three sides to the exterior. Therefore, we need to consider atmost 4 extra edges between the selected enclosure corner vertices and $v_{\infty}$. These atmost 4 extra edges are known as construction edges [6]. A PTG where enclosure vertices are connected to $v_{\infty}$ together with 4 additional construction edges is called an EPTG (extended planar triangulated graph). An EPTG is depicted in Fig. [5](#S3.F5) by red edges.

It is interesting to note that the regions including unbounded region are triangulated in EPTG so that every region including unbounded region of the dual of RDG is quadrangle. This permits the enclosure to be rectangular. A detailed description of unbounded quadrangle region of the dual can be seen in Section [4](#S4). Since there is one to one corresponding between the edges of a plane graph and its dual, an enclosure corner vertex has parallel edges to $v_{\infty}$.
In this paper, we consider a simple connected plane triangulated graph, i.e., there is no loops or parallel edges. However, some minor changes ( parallel edges between enclosure corner vertices and $v_{\infty}$ only) in the EPTG is done in order to choose four construction edges.

Figure: Figure 5: Construction of an extended RDG (red edges) and its corresponding RDG (dark edges).
Refer to caption: /html/2102.05304/assets/x5.png

## 4 RDG Stereographic Projection

In this section, we describe stereographic projection of a rectangular graph.

A small difference between a rectangular dual graph and an RFP is that RFP is an edge-oriented graph such that each of its edges is either horizontally or vertically aligned together with rectangular enclosure whereas the edges of a rectangular dual graph may not be oriented either horizontally or vertically, but they can always be oriented horizontally or vertically. Thus, for transforming a rectangular dual graph into be an RFP, we need to know the orientations of the edges. Once it is known, a rectangular dual graph can be converted into an RFP by orienting edges aligned along horizontal or vertical axis of the Euclidean plane. Thus, an RFP can be seen an embedding of a rectangular dual graph. Further it is interesting to note that an RFP is always a rectangular dual graph, but converse it not true.

Let $\mathcal{D}$ be the rectangular dual graph of an RDG $\mathcal{G}$. Note that a connected plane graph is a single piece made up of continuous curves (called edges) joining their ends to pairs of the specified points (called vertices) in the Euclidean plane. Consider a sphere $S$ centered at $(0,0,1/2)$ having radius $1/2$, and a fixed plane embedding $\mathcal{D}^{*}$ of $\mathcal{D}$ in the Euclidean plane passing through $z=0$ ($xy$-plane). Let $(0,0,1)$ be the north pole $N$ and $p$ be a point of an edge of $\mathcal{D}^{*}$. Draw a line segment joining the points $N$ and $p$. Let $t$ be a point where it intersects the surface of $S$. Thus we see that the point $p$ is mapped to the point $t$. In this way, the image of each of its points is a curved line on the surface of $S$ and hence each edge of $\mathcal{D}^{*}$ is mapped to a curved line on the surface of $S$. This results an embedding of $\mathcal{D}^{*}$ on the surface of a sphere.

Now, it is important to identify why the edge of $\mathcal{D}^{*}$ is mapped to the edges on the surface of $S$? In fact, a connected graph is carried to a connected graph by a continuous map. Thus being the mapping continuous, the image of $\mathcal{D}^{*}$ is again a plane graph on the surface of $S$ with its exterior bounded. Note that the unbounded region is now mapped into a bounded region on $S$ passing through $N$. This process is known as stereographic projection and sphere is known as Riemann sphere. But $\mathcal{D}^{*}$ is a rectangular dual graph. Its exterior is a four sided rectangular enclosure. This results the unbounded region of $\mathcal{D}^{*}$ corresponds to a four sided bounded region of the corresponding plane graph embedded on the surface of $S$. Consequently, when we assign horizontal or vertical orientations to the edges of $\mathcal{D}^{*}$ to transform into an RFP, the unbounded region of $\mathcal{D}^{*}$ corresponds to an unbounded rectangle (region) $R_{\infty}$ passing through $\infty$. Thus we see that the exterior of an RFP is a rectangle $R_{\infty}$ passing through $\infty$. Note that $R_{\infty}$ is not a part of an RFP, but is a rectangle that shares its two adjacent sides to each of its enclosure corner rectangles. Recall that a rectangle is a four-sided region with 4 right interior angles formed by its sides. Although in case of $R_{\infty}$, these interior angles can be realized to be $90^{\circ}$ by looking at it from a point at $\infty$, otherwise we realize every interior angle to be $270^{\circ}$. The role of the point at $\infty$ is played by $N$ and hence an alternative way is to realize right angle between two sides of the four-sided region passing through $N$ in the stereographic projection of the rectangular dual graph is the angle between the intersection of their tangents to the sides of this region. This discussion realizes us that an RFP is quadrangulation of the Euclidean plane.

## 5 RDG Existence Theory

In this section, we describe the theory of RDGs.

###### Theorem 5.1 .

A necessary and sufficient condition for an EPTG $\mathcal{G}^{*}$ to be an RDG is that it is 4-connected and has atmost 4 critical separating triangles passing through $v_{\infty}$.

###### Proof.

Necessary Condition. Assume that $\mathcal{G}^{*}$ is an RDG. Then it has a rectangular dual graph $\mathcal{D}$. Let $v_{i}$ be a vertex of $\mathcal{G}^{*}$ dual to some interior region $R_{i}$ of $\mathcal{D}$. Since every region of $\mathcal{D}$ is four-sided, atleast 4 regions are required to fully enclose an interior region of $\mathcal{D}$. This implies that $R_{i}$ is surrounded by atleast 4 regions of $\mathcal{D}$ and hence $v_{i}$ is adjacent to atleast 4 vertices of $\mathcal{G}^{*}$, i.e., $d(v_{i})\geq 4$. Let $v_{e}$ be a vertex of $\mathcal{G}^{*}$ dual to an enclosure (exterior) region $R_{e}$ of $\mathcal{D}$. There arise two possibilities:

- 1.
$R_{e}$ surrounds exactly its two sides with $R_{\infty}$ if it is an enclosure corner region,
- 2.
$R_{e}$ surrounds exactly its one side with $R_{\infty}$ if it is not an enclosure corner region.

In the first case, $R_{\infty}$ surrounds the two sides of $R_{e}$. There are two edges between $v_{\infty}$ and $v_{e}$ where $v_{\infty}$ corresponds to $R_{\infty}$. The remaining two sides of $R_{e}$ are surrounded by atleast two interior regions other than $R_{\infty}$. This implies that $d(v_{e})\geq 4$. In the second case, only one side of $R_{e}$ is surrounded by $R_{\infty}$ and the remaining sides are surrounded by atleast three interior regions. This implies that $d(v_{e})\geq 4$. Since $v_{e}$ and $v_{i}$ are arbitrary vertices of $\mathcal{G}^{*}$, $\mathcal{G}^{*}$ is 4-connected. This proves the first condition.

As discussed in Section [4](#S4), $\mathcal{R}_{\infty}$ surrounds exactly its two adjacent sides to each of the enclosure corner regions of $\mathcal{D}$ and exactly one side to the remaining exterior regions of $\mathcal{D}$. Let $v_{c}$ be a vertex of $\mathcal{G}^{*}$ dual to an enclosure corner region of $\mathcal{D}$. We have already shown that $\mathcal{G}^{*}$ is 4-connected, i.e., $d(v_{c})\geq 4$, $\forall v_{c}\in\mathcal{G}^{*}$. If $d(v_{c})=4$, then two adjacent sides of $R_{c}$ are surrounded by $R_{\infty}$ whereas the remaining two sides of $R_{c}$ are surrounded by two regions $R_{a}$ and $R_{b}$. Clearly, $R_{a}$ and $R_{b}$ are the enclosure regions. Since $\mathcal{G}^{*}$ is an EPTG, every region of $\mathcal{G}^{*}$ is triangular. This implies that $R_{a}$ and $R_{b}$ are adjacent. Consequently, there is a separating triangle passing through $v_{\infty}$ and vertices that are dual to $R_{a}$ and $R_{b}$. Clearly, it encloses exactly one vertex $v_{c}$. This implies that there is no separating triangle inside this separating triangle and hence it is a critical separating triangle. This situation is depicted in Fig. [6](#S5.F6)a. If $d(v_{c})>4$, there are atleast three interior regions that surround $R_{c}$. Vertices that are dual to these interior regions together with $v_{\infty}$ is a cycle of length atleast 4. Only possibility for the existence of a critical separating triangle passing through $v_{\infty}$ and enclosing $v_{c}$ is depicted in Fig. [6](#S5.F6)b. Now it is evident that there is atmost one critical separating triangle passing through $v_{\infty}$ corresponding to each enclosure corner region. Since a rectangular graph has atmost four enclosure corner regions, there can be atmost 4 critical separating triangles passing through $v_{\infty}$. This proves the second condition.

Figure: Figure 6: Two possibilities of a critical separating triangle enclosing an enclosure corner vertex.
Refer to caption: /html/2102.05304/assets/x6.png

Sufficient Condition. Assume that the given conditions hold.
We prove the result by applying the induction method on the vertices of $\mathcal{G}^{*}$. Recall that an EPTG contains atleast two vertices. Let $n$ be the number of vertices of $\mathcal{G}^{*}$. If $n=2$, then it is a graph consisting of a single edge and hence it is an RDG. Let us assume that $n>2$ and the result holds for $n-1$ vertices, i.e., every $(n-1)$-vertex EPTG satisfying the given conditions is an RDG. In order to complete induction, we need to prove that $n$-vertex EPTG $\mathcal{H}$ satisfying the given conditions is an RDG. Since there can be atmost four critical separating triangles in $\mathcal{H}$, there arise two possibilities: (1) there are exactly three edges between $v_{\infty}$ and atleast one of the enclosure vertices, (2) there are exactly two edges between $v_{\infty}$ and each enclosure corner vertex. Let $v_{i}$ be an enclosure corner vertex of $\mathcal{H}$ and $A=\{v_{1},v_{2},\dots,v_{t}\}$ be the set of vertices adjacent to $v_{i}$.

Consider the first case, i.e., there exist edges $(v_{i},v_{\infty})$, $(v_{i},v_{p})$, $(v_{i},v_{q})$ where vertices $v_{p}$ and $v_{q}$ are incident to $v_{\infty}$ as shown in Fig. [7](#S5.F7)a. Construct a new EPTG $\mathcal{H}_{1}$ by deleting $v_{i}$ together with the incident edges and introducing new edges $(v_{\infty},v_{1})$, $(v_{\infty},v_{2})\dots(v_{\infty},v_{t})$ (see Fig. [7](#S5.F7)b). We prove that $\mathcal{H}_{1}$ satisfies the given conditions stated in the theorem.

Consider two vertices $v_{a}$ and $v_{b}$ of $\mathcal{H}$ such that $i\neq a,b$. As $\mathcal{H}$ is 4-connected, by Menger’s theorem, there exist four vertex-disjoint paths between $v_{a}$ and $v_{b}$. Choose each path of the shortest possible length. If none of these paths uses the edges $(v_{i},v_{p})$ and $(v_{i},v_{q})$, then the same path would exist in $\mathcal{H}_{1}$ with the edge $(v_{\infty},v_{k})$, $(1\leq k\leq t)$ substituted in the place of $(v_{k},v_{i})\cup(v_{i},v_{\infty})$ if they occur in the path. Otherwise suppose that one of the four paths passes through $(v_{i},v_{p})$. Being the shortest possible path, it can not pass through $v_{\infty}$ or $v_{k}$, $(1\leq k\leq t)$. Consequently, it must use the edge $(v_{i},v_{q})$. If a path passes through $v_{\infty}$, it would pass through $v_{p}$ or $v_{q}$, contradicting to the facts that path is the shortest. Thus vertex $v_{\infty}$ is not used by any of the four paths. Now by substituting the part $(v_{i},v_{p})\cup(v_{i},v_{q})$ of the path in $\mathcal{H}$ by $(v_{p},v_{\infty})\cup(v_{\infty},v_{q})$ in $\mathcal{H}_{1}$, we can obtain 4 vertex-disjoint paths in $\mathcal{H}_{1}$ also. Then by Menger’s theorem, $\mathcal{H}_{1}$ is 4-connected.

Next we claim that the number of critical separating triangles in $\mathcal{H}_{1}$ can not be more than the number of critical separating triangles in $\mathcal{H}$. As discussed in the necessary part that there is atmost one critical separating triangle enclosing an enclosure corner vertex and $\mathcal{H}$ has three enclosure corner vertices, there are atmost three critical separating triangles in $\mathcal{H}$. Then the only possibility of occurring a separating triangle in $\mathcal{H}_{1}$ is as follows. If an enclosure vertex $v_{l}$ is incident to both $v_{p}$, $v_{k}$ where $v_{k}\in A$, then there exists a separating triangle in $\mathcal{H}_{1}$ passing through $v_{k}$, $v_{p}$ and $v_{l}$. Similarly, there can be another separating triangle in $\mathcal{H}_{1}$ passing through $v_{t}$, $v_{q}$ and $v_{s}\in A$. thus there can be atmost two new separating triangles in $\mathcal{H}_{1}$. If there exists a critical separating triangle $T_{c}$ containing $v_{i}$ in $\mathcal{H}$, then there are three possibilities: (1) there no longer remains $T_{c}$ in $\mathcal{H}_{1}$, (2) $T_{c}$ is contained in one of the new created separating triangles in $\mathcal{H}_{1}$, and (3) One of the new created separating triangle is contained in $T_{c}$. All these possibilities show that there can not be more than four critical separating triangles in $\mathcal{H}_{1}$. This shows that $\mathcal{H}_{1}$ has atmost 4 critical separating triangles. Thus, $\mathcal{H}_{1}$ has $n-1$ vertices satisfying the given conditions. By induction hypothesis, $\mathcal{H}_{1}$ is an RDG and hence admits an RFP. This RFP can be transformed to another RFP by adjoining a region $R_{i}$ (corresponding to $v_{i}$) as shown in Fig. [7](#S5.F7)c. Then the resultant RFP corresponds to $\mathcal{H}$. Hence $\mathcal{H}$ is an RDG.

Consider the second case. In this case, $\mathcal{H}$ appears as shown in Fig. [8](#S5.F8)a with atleast four more vertices $v_{1},v_{2},v_{3}$ and $v_{4}$. Consider the four enclosure corner vertices $v_{1}$, $v_{2}$, $v_{3}$ and $v_{4}$ as shown in Fig. [8](#S5.F8)a. Now we show that there is a separating cycle $C$ passing through $v_{i}$, $v_{\infty}$ and an enclosure vertex $v_{d}$ but not passing through $v_{3}$ or $v_{4}$ such that the removal of vertices of $C$ from $\mathcal{H}$ disconnects it into two connected pieces, each containing atleast one vertex.

If there is an edge $(v_{1},v_{3})$ in $\mathcal{H}$, there is a separating cycle passing through $v_{1},v_{3}$ and $v_{\infty}$. In this case, $\mathcal{H}$ is separated into two parts, one of which contains atleast $v_{2}$ and another contains atleast $v_{4}$.

Figure: Figure 7: (a) Sketch of the graph $\mathcal{H}$ when there are three edges between $v_{\infty}$ and enclosure vertex $v_{i}$, (b) sketch of the graph $\mathcal{H}_{1}$ when there are exactly two edges between each enclosure corner vertex and $v_{\infty}$, and (c) the construction of an rectangular dual for $\mathcal{H}$.
Refer to caption: /html/2102.05304/assets/x7.png

If there is no edge $(v_{1},v_{3})$ in $\mathcal{H}$. All vertices adjacent to $v_{3}$ lie on a path $y_{1}y_{2}\dots y_{k}$ where $y_{1}$ and $y_{k}$ are the enclosure vertices. Let $y_{k}x_{1}x_{2}\dots v_{2}$ be a path of the enclosure vertices starting from $y_{k}$ and ending with $v_{2}$. Then $C=ty_{1}y_{2}\dots y_{k}x_{1}x_{2}\dots v_{2}$ is a separating cycle which separates $\mathcal{H}$ into two parts, one of which atleast contains $v_{1}$ and another contains atleast $v_{3}$.

Figure: Figure 8: (a) A separating cycle shown by red edges and (b) the appearance of $\mathcal{H}_{u}$.
Refer to caption: /html/2102.05304/assets/x8.png

Once a separating cycle exists, there also exists the shortest separating cycle $C_{s}=v_{\infty}z_{1}z_{2}\dots z_{m}z_{m+1}$. This situation is depicted in [8](#S5.F8)b. Without loss of generality, suppose $C_{s}$ separates $v_{1}$ and $v_{3}$. Construct an EPTG $\mathcal{H}_{u}$ from the subgraph contained in the interior of $C$ by adding a vertex $v_{\infty}$ and edges between $v_{\infty}$ and enclosure vertices of this subgraph. The new edges in this construction are $(v^{\prime}_{\infty},z_{1})$, $(v^{\prime}_{\infty},z_{2})$, …$(v^{\prime}_{\infty},z_{m+1})$. Now we show that $\mathcal{H}_{u}$ satisfies the given conditions. Only possibility for creating a separating triangle is a triangle $z_{i}z_{i+1}v_{\infty}$ for $1\leq i\leq n$. If there would exist an edge $(z_{i},z_{i+1})$ in $\mathcal{H}_{u}$, then it contradicts that $C_{s}$ is the shortest separating cycle. Therefore, any cycle in $\mathcal{H}_{u}$ is of length atleast 4 and consequently, $\mathcal{H}_{u}$ is 4-connected and can not have more than 4 separating triangles. By induction hypothesis, $\mathcal{H}_{u}$ is an RDG. Similarly, we can show that the EPTG $\mathcal{H}_{b}$ constructed from the the remaining part of $\mathcal{H}$ is an RDG. Then the corresponding RFP can be placed one above the other and can be merged after applying homeomorphic transformation so as to preserve orthogonal directions of the edges such that the resultant floorplan is an RFP of $\mathcal{H}$ as shown in Fig. [9](#S5.F9). This completes the induction process and hence completes the proof.
∎

Figure: Figure 9: (a) Merging two RDGs of $\mathcal{H}_{u}$ and $\mathcal{H}_{b}$ into an RDG for $\mathcal{H}$
Refer to caption: /html/2102.05304/assets/x9.png

Now we turn our attention to derive a necessary and sufficient condition for a PTG to be an RDG. A plane graph can be either nonseparable graph (block) or a separable connected graph. A disconnected graph is also a separable graph. However, we are not considering this case since RFP are concerned with connectivity.

###### Theorem 5.2 .

A necessary and sufficient condition for a nonseparable PTG $\mathcal{G}$ to an RDG is that it is 4-connected and has atmost 4 critical shortcuts.

###### Proof.

Necessary Condition. Assume that $\mathcal{G}$ is an RDG. Then it admits an RFP $\mathcal{F}$. Let $v_{i}$ be an interior vertex of $\mathcal{G}$ dual to a rectangular region $R_{i}$ of $\mathcal{F}$. Recall that there require atleast 4 component rectangular regions to surround a rectangular region in an RFP. Therefore, there exist atleast 4 rectangular regions in $\mathcal{F}$ enclosing $R_{i}$. Then $v_{i}$ is adjacent to atleast 4 vertices, i.e., $d(v_{i})\geq 4$. Since $v_{i}$ is an arbitrary interior vertex of $\mathcal{G}$, $\mathcal{G}$ is 4-connected.

To the contrary, if there exist 5 critical shortcuts in $\mathcal{G}$, the corresponding EPTG $\mathcal{G}^{*}$ would contain 5 critical separating triangles, each passing through exactly one critical shortcut. This is a contradiction to Theorem [5.1](#S5.Thmtheorem1). This shows that $\mathcal{G}$ can not have more than 4 critical shortcuts.

Sufficient Condition. Assume that the given conditions hold.
Choose 4 enclosure corner vertices, each on the path joining the endpoints of the critical shortcut lying on its outermost cycle but not as the endpoints of these paths. If the number of critical shortcuts are less than 4, choose the remaining enclosure corner vertices randomly among enclosure vertices. Join each of these 4 vertices to $v_{\infty}$ by two parallel edges and join each of the remaining $n-4$ enclosure vertices to $v_{\infty}$ by a single edge. This constructs an EPTG $\mathcal{G}^{*}$ satisfying all the conditions given in Theorem [5.1](#S5.Thmtheorem1). Hence $\mathcal{G}$ is an RDG. This completes the proof.
∎

###### Theorem 5.3 .

A necessary and sufficient condition for a separable connected PTG $\mathcal{G}$ to be an RDG is that:

- 1.
each of its blocks is 4-connected,
- 2.
BNG is a path,
- 3.
both endpoints of an exterior edge of each of its blocks are not cut vertices,
- 4.
each maximal blocks corresponding to the endpoints of
the BNG contains at most 2 critical shortcuts, not passing through cut vertices,
- 5.
Other remaining maximal blocks do not contain a critical shortcut, not passing through a cut vertex.

###### Proof.

Necessary Condition. Assume that $\mathcal{G}$ is an RDG. The proof of the first condition is a direct consequence followed by Theorem [5.1](#S5.Thmtheorem1). The BNG of $\mathcal{G}$ has the following possibilities:

- 1.
it can be path,
- 2.
it can be a cycle of length $\geq 3$,
- 3.
it can be a tree.

To the contrary, suppose that the BNG is a cycle of length atleast $3$. This implies that atleast three blocks share some cut vertex $v_{c}$ of $\mathcal{G}$. The construction of an EPTG $\mathcal{G}^{*}$ create more than 4 critical separating triangles, each passing through $v_{c}$, $v_{\infty}$, and a vertex adjacent to $v_{c}$ that belongs to the outermost cycle of each block. This situation can be depicted in Fig. [10](#S5.F10)a. Then by Theorem [5.1](#S5.Thmtheorem1), $\mathcal{G}$ no longer is an RDG. A similar argument can be applied when it is a tree. This situation can be depicted in Fig. [11](#S5.F11)a. Thus, the BNG is left with one possibility, i.e., the BNG is a path.

To the contrary, suppose that both the endpoints of an exterior edge $(v_{i},v_{j})$ of a block are cut vertices, then there are more than 4 critical separating triangles passing through $v_{i}$, $v_{j}$ and $v_{\infty}$ in $\mathcal{G}^{*}$, which is a contradiction to Theorem [5.1](#S5.Thmtheorem1). Hence both the endpoints of an exterior edge of a block can not be cut vertices simultaneously.

Let $M_{i}$ be a maximal block corresponding to the endpoints of
the BNG. Since $\mathcal{G}$ is an RDG, each of its block is an RDG. Suppose that $M_{i}$ is an RDG. Then it admits an RFP $\mathcal{F}_{i}$. It can be easily noted that out of 4 corner rectangular regions of $\mathcal{F}_{i}$, only two can be the corner rectangular regions of $\mathcal{F}$. Then there can be atmost two critical separating triangles in $\mathcal{G}^{*}$ and hence there can be atmost two critical shortcuts in each $M_{i}$. This implies that the second condition holds. Also, any other maximal block of the BNG can not share critical separating triangles since any corner rectangular region in $\mathcal{F}$ is an RFP. This implies that no other maximal block has a critical separating triangle in $\mathcal{G}^{*}$ and hence there is no critical shortcut in the remaining maximal blocks.

Figure: Figure 10: (a) A separable connected graph constituted by three blocks A, B and C, and (b) its BNG. Here only the outermost cycles of the blocks are shown.
Refer to caption: /html/2102.05304/assets/x10.png

Sufficient Condition. Assume that the given conditions hold. The first condition shows that $\mathcal{G}^{*}$ is 4-connected.
The remaining conditions show that there are atmost four critical separating triangles in $\mathcal{G}^{*}$. By Theorem [5.1](#S5.Thmtheorem1), $\mathcal{G}$ is an RDG. Hence the proof.
∎

Figure: Figure 11: A separable connected graph constituted by three blocks A, B, C and D, and (b) its BNG. Here only the outermost cycles of the blocks are shown.
Refer to caption: /html/2102.05304/assets/x11.png

###### Theorem 5.1 .

###### Proof.

###### Theorem 5.2 .

###### Proof.

###### Theorem 5.3 .

###### Proof.

## 6 Concluding remarks and future task

We developed graph theoretic characterization of RFPs. We reported that the existing RDG theory may fail in some cases.
Hence, we proposed a new RDG theory which is easily implementable and it simplifies the floorplan construction process of the VLSI circuits as well architectural buildings.

In future, it would be interesting to transform a nonRDG into an RDG by removing those edges which violates the RDG property and then adding new edges (maintaining RDG property) in such a way that the distances of endpoints of the deleted edges can be minimized. This idea would be useful in reducing the interconnection wire-lengths as well as in complex buildings, it gives the shortest possible paths for those pairs of rooms which is impossible to directly connect.

## References

- [1]
K. Koźmiński, E. Kinnen, Rectangular duals of planar graphs, Networks
15 (2) (1985) 145–157.
- [2]
K. Maling, W. Heller, S. Mueller, On finding most optimal rectangular package
plans, in: 19th Design Automation Conference, IEEE, 1982, pp. 663–670.
- [3]
K. Shekhawat, Enumerating generic rectangular floor plans, Automation in
Construction 92 (2018) 151–165.
- [4]
J. Bhasker, S. Sahni, A linear time algorithm to check for the existence of a
rectangular dual of a planar triangulated graph, Networks 17 (3) (1987)
307–317.
- [5]
I. Rinsma, Nonexistence of a certain rectangular floorplan with specified areas
and adjacency, Environment and Planning B: Planning and Design 14 (2) (1987)
163–166.
- [6]
Y.-T. Lai, S. M. Leinwand, A theory of rectangular dual graphs, Algorithmica
5 (1-4) (1990) 467–483.
- [7]
K. Kozminski, E. Kinnen, An algorithm for finding a rectangular dual of a
planar graph for use in area planning for vlsi integrated circuits, in: 21st
Design Automation Conference Proceedings, IEEE, 1984, pp. 655–656.
- [8]
J. Bhasker, S. Sahni, A linear algorithm to find a rectangular dual of a planar
triangulated graph, Algorithmica (1988) 247–278.
- [9]
Y.-T. Lai, S. M. Leinwand, Algorithms for floorplan design via rectangular
dualization, IEEE transactions on computer-aided design of integrated
circuits and systems 7 (12) (1988) 1278–1289.
- [10]
K. A. Kozminski, E. Kinnen, Rectangular dualization and rectangular
dissections, IEEE Transactions on Circuits and Systems 35 (11) (1988)
1401–1416.
- [11]
H. Tang, W.-K. Chen, Generation of rectangular duals of a planar triangulated
graph by elementary transformations, in: IEEE International Symposium on
Circuits and Systems, IEEE, 1990, pp. 2857–2860.
- [12]
X. He, On finding the rectangular duals of planar triangular graphs, SIAM
Journal on Computing 22 (6) (1993) 1218–1226.
- [13]
G. K. Yeap, M. Sarrafzadeh, Sliceable floorplanning by graph dualization, SIAM
Journal on Discrete Mathematics 8 (2) (1995) 258–280.
- [14]
G. Kant, X. He, Regular edge labeling of 4-connected plane graphs and its
applications in graph drawing problems, Theoretical Computer Science
172 (1-2) (1997) 175–193.
- [15]
P. S. Dasgupta, S. Sur-Kolay, B. B. Bhattacharya, A unified approach to
topology generation and optimal sizing of floorplans, IEEE transactions on
computer-aided design of integrated circuits and systems 17 (2) (1998)
126–135.
- [16]
P. Dasgupta, S. Sur-Kolay, Slicible rectangular graphs and their optimal
floorplans, ACM Transactions on Design Automation of Electronic Systems 6 (4)
(2001) 447–470.
- [17]
D. Eppstein, E. Mumford, B. Speckmann, K. Verbeek, Area-universal and
constrained rectangular layouts, SIAM Journal on Computing 41 (3) (2012)
537–564.
- [18]
S.-i. Nakano, Enumerating floorplans with n rooms, in: International Symposium
on Algorithms and Computation, Springer, 2001, pp. 107–115.
- [19]
Z. C. Shen, C. C. Chu, Bounds on the number of slicing, mosaic, and general
floorplans, IEEE Transactions on Computer-Aided Design of Integrated Circuits
and Systems 22 (10) (2003) 1354–1361.
- [20]
B. Yao, H. Chen, C.-K. Cheng, R. Graham, Floorplan representations: Complexity
and connections, ACM Transactions on Design Automation of Electronic Systems
(TODAES) 8 (1) (2003) 55–80.
- [21]
E. Ackerman, G. Barequet, R. Y. Pinter, A bijection between permutations and
floorplans, and its applications, Discrete Applied Mathematics 154 (12)
(2006) 1674–1684.
- [22]
N. Reading, Generic rectangulations, European Journal of Combinatorics 33 (4)
(2012) 610–623.
- [23]
B. D. He, A simple optimal binary representation of mosaic floorplans and
baxter permutations, Theoretical Computer Science 532 (2014) 40–50.
- [24]
K. Yamanaka, M. S. Rahman, S.-I. Nakano, Floorplans with columns, in:
International Conference on Combinatorial Optimization and Applications,
Springer, 2017, pp. 33–40.
- [25]
K.-H. Yeap, M. Sarrafzadeh, Floor-planning by graph dualization: 2-concave
rectilinear modules, SIAM Journal on Computing 22 (3) (1993) 500–526.
- [26]
X. He, On floor-plan of plane graphs, SIAM Journal on Computing 28 (6) (1999)
2150–2167.
- [27]
Y.-T. Chiang, C.-C. Lin, H.-I. Lu, Orderly spanning trees with applications,
SIAM Journal on Computing 34 (4) (2005) 924–945.
- [28]
H. Zhang, S. Sadasivam, Improved floor-planning of graphs via
adjacency-preserving transformations, Journal of combinatorial optimization
22 (4) (2011) 726–746.
- [29]
M. J. Alam, T. Biedl, S. Felsner, M. Kaufmann, S. G. Kobourov, T. Ueckerdt,
Computing cartograms with optimal complexity, Discrete & Computational
Geometry 50 (3) (2013) 784–810.
- [30]
I. Rinsma, Existence theorems for floorplans, Bulletin of the Australian
Mathematical Society 37 (3) (1988) 473–475.
- [31]
D. B. West, et al., Introduction to graph theory, Vol. 2, Prentice hall Upper
Saddle River, NJ, 1996.
