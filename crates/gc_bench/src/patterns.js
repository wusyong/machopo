"use strict";
// gc_bench's allocation patterns on V8's heap: groups of objects linked as
// `gc_heap::new_group` links them on the gc crate's, object for object and
// with the same choices (`Rng32` in lib.rs), so both heaps hold the same
// graph. Each object has its edges and two numbers of data.

class Object1 {
  constructor() {
    this.e0 = null;
    this.d0 = 0;
    this.d1 = 0;
  }
}

class Object2 {
  constructor() {
    this.e0 = null;
    this.e1 = null;
    this.d0 = 0;
    this.d1 = 0;
  }
}

class Object4 {
  constructor() {
    this.e0 = null;
    this.e1 = null;
    this.e2 = null;
    this.e3 = null;
    this.d0 = 0;
    this.d1 = 0;
  }
}

let seed = 0;
// xorshift32, as `Rng32::below`: the operators work on the 32 bits, and
// `>>> 0` reads them as unsigned.
function below(bound) {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  seed >>>= 0;
  return seed % bound;
}

let edges = 0;
let groupSize = 0;
let groupsPerTurn = 0;
let keptPercent = 0;
let root = null;
let attach = null;
// A group's objects while it is built (Rust keeps them in a `Vec`): one
// array for the run, emptied after each group.
let scratch = [];
// The group just built: what reaches it all, and the object whose edge 0
// is free for the next group.
let entry = null;
let last = null;

function newObject() {
  return edges === 1 ? new Object1() : edges === 2 ? new Object2() : new Object4();
}

// `Shape::Chain`: object i's e0 is object i + 1.
function chain() {
  entry = new Object1();
  last = entry;
  for (let i = 1; i < groupSize; i++) {
    const object = new Object1();
    last.e0 = object;
    last = object;
  }
}

// `Shape::BinaryTree`: breadth first, object i's e0 and e1 are objects
// 2i + 1 and 2i + 2.
function tree() {
  for (let i = 0; i < groupSize; i++) {
    const object = new Object2();
    scratch[i] = object;
    if (i > 0) {
      const parent = scratch[(i - 1) >> 1];
      if ((i - 1) % 2 === 0) {
        parent.e0 = object;
      } else {
        parent.e1 = object;
      }
    }
  }
  entry = scratch[0];
  last = scratch[groupSize - 1];
  scratch.fill(null);
}

// `Pattern::Graph`: a chain through e0, then every object's other
// edges to objects of the group picked at random, object by object.
function graph() {
  for (let i = 0; i < groupSize; i++) {
    const object = new Object4();
    scratch[i] = object;
    if (i > 0) {
      scratch[i - 1].e0 = object;
    }
  }
  for (let i = 0; i < groupSize; i++) {
    const object = scratch[i];
    object.e1 = scratch[below(groupSize)];
    object.e2 = scratch[below(groupSize)];
    object.e3 = scratch[below(groupSize)];
  }
  entry = scratch[0];
  last = scratch[groupSize - 1];
  scratch.fill(null);
}

let build = chain;

function setup(edgeCount, size, groups, percent, initialSeed) {
  edges = edgeCount;
  groupSize = size;
  groupsPerTurn = groups;
  keptPercent = percent;
  seed = initialSeed;
  build = edges === 1 ? chain : edges === 2 ? tree : graph;
  scratch = new Array(groupSize).fill(null);
  root = newObject();
  attach = root;
}

// One turn: `groupsPerTurn` new groups, each kept (linked from the last
// kept one) or dropped.
function turn() {
  for (let g = 0; g < groupsPerTurn; g++) {
    build();
    if (below(100) < keptPercent) {
      attach.e0 = entry;
      attach = last;
    }
  }
  entry = null;
  last = null;
}

// The objects the root reaches, itself included.
function count() {
  const seen = new Set([root]);
  const left = [root];
  while (left.length > 0) {
    const object = left.pop();
    for (const next of [object.e0, object.e1, object.e2, object.e3]) {
      if (next && !seen.has(next)) {
        seen.add(next);
        left.push(next);
      }
    }
  }
  return seen.size;
}

function reset() {
  root = null;
  attach = null;
  scratch = [];
}
