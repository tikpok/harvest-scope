import { useEffect, useMemo, useRef, useState } from "react";
import { RotateCcw, Route, X } from "lucide-react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { NetworkEvent, TopologyEdge, TopologyNode } from "../types";

export interface TopologySpaceExplorerProps {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
  events: NetworkEvent[];
  now: number;
  wideView: boolean;
  selectedNodeId: string | null;
  onInspectNode: (nodeId: string) => void;
  onClose: () => void;
}

interface PlanetRecord {
  node: TopologyNode;
  mesh: THREE.Mesh;
  baseScale: number;
}

interface ParticleRecord {
  mesh: THREE.Mesh;
  source: THREE.Vector3;
  target: THREE.Vector3;
  observedAt: number;
}

interface SceneData {
  signature: string;
  nodes: TopologyNode[];
  edges: TopologyEdge[];
}

interface CameraState {
  position: THREE.Vector3;
  target: THREE.Vector3;
}

const eventLabels: Record<NetworkEvent["kind"], string> = {
  block: "ブロック高更新",
  finalizedBlock: "ファイナライズ高更新",
  pollUpdate: "ブロック高更新",
  harvestedBlock: "ハーベスト検知",
};

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
const MAX_DEVICE_PIXEL_RATIO = 1.35;

function performanceProfile(nodeCount: number, edgeCount: number) {
  const dense = nodeCount >= 90 || edgeCount >= 240;
  const veryDense = nodeCount >= 150 || edgeCount >= 420;
  return {
    dense,
    starCount: veryDense ? 260 : dense ? 420 : 700,
    sphereWidthSegments: veryDense ? 18 : dense ? 24 : 32,
    sphereHeightSegments: veryDense ? 12 : dense ? 16 : 20,
    ringSegments: veryDense ? 28 : dense ? 40 : 52,
    maxEventParticles: veryDense ? 14 : dense ? 22 : 30,
    maxEdgesPerEvent: veryDense ? 2 : dense ? 3 : 4,
    activeFps: veryDense ? 24 : 30,
    idleFps: veryDense ? 12 : 18,
  };
}

function supportsWebGl() {
  if (typeof window === "undefined" || navigator.userAgent.includes("jsdom")) return false;
  try {
    const canvas = document.createElement("canvas");
    return Boolean(canvas.getContext("webgl2") ?? canvas.getContext("webgl"));
  } catch {
    return false;
  }
}

function hashValue(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) hash = ((hash << 5) - hash + value.charCodeAt(index)) | 0;
  return Math.abs(hash);
}

function nodeColor(node: TopologyNode) {
  if (node.status === "healthy") return 0x55d6be;
  if (node.status === "warning") return 0xffca5c;
  if (node.status === "critical" || node.status === "offline") return 0xff6b86;
  return node.registered ? 0x73a9ff : 0x7d8fa6;
}

function edgeColor(status: string) {
  if (status === "active") return 0x55d6be;
  if (status === "busy") return 0xffca5c;
  if (status === "failure") return 0xff6b86;
  return 0x52667c;
}

function eventColor(kind: NetworkEvent["kind"]) {
  if (kind === "harvestedBlock") return 0xf5d36f;
  if (kind === "finalizedBlock") return 0xd58cff;
  return 0x73a9ff;
}

function toSpacePosition(node: TopologyNode, index: number, count: number) {
  const normalized = count <= 1 ? 0 : index / (count - 1);
  const y = 1 - normalized * 2;
  const horizontalRadius = Math.sqrt(Math.max(0, 1 - y * y));
  const angle = index * GOLDEN_ANGLE + (hashValue(node.id) % 360) * (Math.PI / 180);
  const shellRadius = (node.registered ? 24 : 34) + (hashValue(`${node.id}:shell`) % 900) / 100;
  return new THREE.Vector3(
    Math.cos(angle) * horizontalRadius * shellRadius,
    y * shellRadius,
    Math.sin(angle) * horizontalRadius * shellRadius,
  );
}

function buildDegree(edges: TopologyEdge[]) {
  const degree = new Map<string, number>();
  edges.forEach((edge) => {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1);
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1);
  });
  return degree;
}

function sceneSignature(nodes: TopologyNode[], edges: TopologyEdge[], wideView: boolean) {
  const nodePart = nodes.map((node) => `${node.id}:${node.registered ? 1 : 0}`).sort().join("|");
  const edgePart = edges.map((edge) => `${edge.id}:${edge.source}:${edge.target}`).sort().join("|");
  return `${wideView ? "wide" : "normal"}::${nodePart}::${edgePart}`;
}

export function TopologySpaceExplorer({
  nodes,
  edges,
  events,
  now,
  wideView,
  selectedNodeId,
  onInspectNode,
  onClose,
}: TopologySpaceExplorerProps) {
  const mountRef = useRef<HTMLDivElement | null>(null);
  const eventsRef = useRef(events);
  const nowRef = useRef(now);
  const selectedNodeIdRef = useRef(selectedNodeId);
  const routesVisibleRef = useRef(false);
  const autoRotateRef = useRef(false);
  const sceneDataRef = useRef<SceneData | null>(null);
  const cameraStateRef = useRef<CameraState | null>(null);
  const [fallback, setFallback] = useState<string | null>(null);
  const [routesVisible, setRoutesVisible] = useState(false);
  const [autoRotate, setAutoRotate] = useState(false);
  const signature = sceneSignature(nodes, edges, wideView);
  if (!sceneDataRef.current || sceneDataRef.current.signature !== signature) {
    sceneDataRef.current = {
      signature,
      nodes,
      edges,
    };
  }
  const sceneData = sceneDataRef.current;
  const eventNodes = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
  const visibleEvents = useMemo(() => events
    .slice()
    .sort((left, right) => right.observedAt - left.observedAt)
    .slice(0, 20), [events]);

  useEffect(() => {
    eventsRef.current = events;
  }, [events]);

  useEffect(() => {
    nowRef.current = now;
  }, [now]);

  useEffect(() => {
    selectedNodeIdRef.current = selectedNodeId;
  }, [selectedNodeId]);

  useEffect(() => {
    routesVisibleRef.current = routesVisible;
  }, [routesVisible]);

  useEffect(() => {
    autoRotateRef.current = autoRotate;
  }, [autoRotate]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  useEffect(() => {
    const mount = mountRef.current;
    if (!mount) return undefined;
    const sceneNodes = sceneData.nodes;
    const sceneEdges = sceneData.edges;
    mount.replaceChildren();
    setFallback(null);

    if (!supportsWebGl()) {
      setFallback("3Dビューを初期化できません。WebGLに対応した環境でお試しください。");
      return undefined;
    }

    const profile = performanceProfile(sceneNodes.length, sceneEdges.length);
    const renderer = new THREE.WebGLRenderer({ antialias: !profile.dense, alpha: true, powerPreference: "high-performance" });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, MAX_DEVICE_PIXEL_RATIO));
    renderer.setSize(mount.clientWidth, mount.clientHeight);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    mount.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    scene.fog = new THREE.FogExp2(0x050b14, 0.01);

    const camera = new THREE.PerspectiveCamera(62, mount.clientWidth / Math.max(mount.clientHeight, 1), 0.1, 420);
    if (cameraStateRef.current) camera.position.copy(cameraStateRef.current.position);
    else camera.position.set(0, 12, 88);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.07;
    controls.enablePan = true;
    controls.minDistance = 14;
    controls.maxDistance = 260;
    controls.autoRotate = false;
    if (cameraStateRef.current) controls.target.copy(cameraStateRef.current.target);

    const ambient = new THREE.AmbientLight(0x9bbcff, 0.55);
    const key = new THREE.PointLight(0x88d8ff, 2.1, 220);
    key.position.set(-30, 34, 36);
    const warm = new THREE.PointLight(0xf5d36f, 1.1, 150);
    warm.position.set(38, -18, -30);
    scene.add(ambient, key, warm);

    const degree = buildDegree(sceneEdges);
    const nodePositions = new Map<string, THREE.Vector3>();
    const orderedNodes = [...sceneNodes].sort((left, right) => hashValue(left.id) - hashValue(right.id));
    orderedNodes.forEach((node, index) => {
      nodePositions.set(node.id, toSpacePosition(node, index, orderedNodes.length));
    });

    const starGeometry = new THREE.BufferGeometry();
    const starValues: number[] = [];
    for (let index = 0; index < profile.starCount; index += 1) {
      const seed = hashValue(`star-${index}`);
      const radius = 82 + (seed % 5200) / 100;
      const theta = ((seed % 360) / 360) * Math.PI * 2;
      const phi = (((seed >> 3) % 180) / 180) * Math.PI;
      starValues.push(
        Math.sin(phi) * Math.cos(theta) * radius,
        Math.cos(phi) * radius,
        Math.sin(phi) * Math.sin(theta) * radius,
      );
    }
    starGeometry.setAttribute("position", new THREE.Float32BufferAttribute(starValues, 3));
    const stars = new THREE.Points(starGeometry, new THREE.PointsMaterial({ color: 0xbfd8ff, size: 0.09, transparent: true, opacity: 0.65 }));
    scene.add(stars);

    const edgeGroup = new THREE.Group();
    const activeEdgesByNode = new Map<string, TopologyEdge[]>();
    sceneEdges.forEach((edge) => {
      const source = nodePositions.get(edge.source);
      const target = nodePositions.get(edge.target);
      if (!source || !target) return;
      if (edge.status === "active") {
        const sourceEdges = activeEdgesByNode.get(edge.source) ?? [];
        sourceEdges.push(edge);
        activeEdgesByNode.set(edge.source, sourceEdges);
        const targetEdges = activeEdgesByNode.get(edge.target) ?? [];
        targetEdges.push(edge);
        activeEdgesByNode.set(edge.target, targetEdges);
      }
      const geometry = new THREE.BufferGeometry().setFromPoints([source, target]);
      const material = new THREE.LineBasicMaterial({
        color: edgeColor(edge.status),
        transparent: true,
        opacity: edge.status === "active" ? 0.58 : 0.28,
      });
      edgeGroup.add(new THREE.Line(geometry, material));
    });
    scene.add(edgeGroup);
    edgeGroup.visible = routesVisibleRef.current;

    const planetGroup = new THREE.Group();
    const planets: PlanetRecord[] = [];
    const planetGeometry = new THREE.SphereGeometry(1, profile.sphereWidthSegments, profile.sphereHeightSegments);
    sceneNodes.forEach((node) => {
      const position = nodePositions.get(node.id);
      if (!position) return;
      const connections = degree.get(node.id) ?? 0;
      const radius = node.registered ? 0.52 + Math.min(connections, 10) * 0.045 : 0.22 + Math.min(connections, 3) * 0.035;
      const color = nodeColor(node);
      const mesh = new THREE.Mesh(planetGeometry, new THREE.MeshStandardMaterial({
        color,
        emissive: color,
        emissiveIntensity: node.registered ? 0.22 : 0.1,
        metalness: 0.38,
        roughness: 0.46,
      }));
      mesh.position.copy(position);
      mesh.scale.setScalar(radius);
      mesh.userData.nodeId = node.id;
      planetGroup.add(mesh);
      planets.push({ node, mesh, baseScale: radius });

      if (node.registered || connections > 1) {
        const ring = new THREE.Mesh(
          new THREE.TorusGeometry(radius * 1.7, 0.012, 8, profile.ringSegments),
          new THREE.MeshBasicMaterial({ color, transparent: true, opacity: node.registered ? 0.45 : 0.24 }),
        );
        ring.position.copy(position);
        ring.rotation.x = Math.PI / 2.8;
        ring.rotation.y = (hashValue(node.id) % 80) / 100;
        planetGroup.add(ring);
      }
    });
    scene.add(planetGroup);

    const selectedRing = new THREE.Mesh(
      new THREE.TorusGeometry(1.4, 0.035, profile.dense ? 8 : 12, profile.ringSegments),
      new THREE.MeshBasicMaterial({ color: 0x73a9ff, transparent: true, opacity: 0.9 }),
    );
    selectedRing.visible = false;
    scene.add(selectedRing);

    const particleGeometry = new THREE.SphereGeometry(0.12, profile.dense ? 8 : 12, profile.dense ? 6 : 8);
    const particles = new Map<string, ParticleRecord>();

    const ensureParticles = () => {
      const currentNow = nowRef.current;
      const liveKeys = new Set<string>();
      eventsRef.current
        .filter((event) => currentNow - event.observedAt < 4_500)
        .slice(0, profile.maxEventParticles)
        .forEach((event) => {
          (activeEdgesByNode.get(event.nodeId) ?? [])
            .slice(0, profile.maxEdgesPerEvent)
            .forEach((edge) => {
              const keyName = `${event.id}-${edge.id}`;
              liveKeys.add(keyName);
              if (particles.has(keyName)) return;
              const sourceId = edge.source === event.nodeId ? edge.target : edge.source;
              const source = nodePositions.get(sourceId);
              const target = nodePositions.get(event.nodeId);
              if (!source || !target) return;
              const mesh = new THREE.Mesh(particleGeometry, new THREE.MeshBasicMaterial({ color: eventColor(event.kind) }));
              scene.add(mesh);
              particles.set(keyName, { mesh, source, target, observedAt: event.observedAt });
            });
        });
      particles.forEach((particle, keyName) => {
        if (liveKeys.has(keyName) || Date.now() - particle.observedAt < 6_000) return;
        scene.remove(particle.mesh);
        particle.mesh.geometry.dispose();
        const material = particle.mesh.material;
        if (Array.isArray(material)) material.forEach((item) => item.dispose());
        else material.dispose();
        particles.delete(keyName);
      });
    };

    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();
    let pointerMoved = false;
    const pointerStart = new THREE.Vector2();

    const onPointerDown = (event: PointerEvent) => {
      pointerMoved = false;
      pointerStart.set(event.clientX, event.clientY);
    };
    const onPointerMove = (event: PointerEvent) => {
      if (pointerStart.distanceTo(new THREE.Vector2(event.clientX, event.clientY)) > 4) pointerMoved = true;
    };
    const onPointerUp = (event: PointerEvent) => {
      if (pointerMoved) return;
      const rect = renderer.domElement.getBoundingClientRect();
      pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
      pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(pointer, camera);
      const hit = raycaster.intersectObjects(planets.map((planet) => planet.mesh), false)[0];
      const nodeId = hit?.object.userData.nodeId;
      if (typeof nodeId === "string") onInspectNode(nodeId);
    };
    renderer.domElement.addEventListener("pointerdown", onPointerDown);
    renderer.domElement.addEventListener("pointermove", onPointerMove);
    renderer.domElement.addEventListener("pointerup", onPointerUp);

    const onResize = () => {
      const width = Math.max(mount.clientWidth, 1);
      const height = Math.max(mount.clientHeight, 1);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
      renderer.setSize(width, height);
    };
    window.addEventListener("resize", onResize);

    let frame = 0;
    let lastFrameAt = 0;
    let lastParticleSyncAt = 0;
    let interactionUntil = 0;
    const onControlsChange = () => {
      interactionUntil = performance.now() + 650;
    };
    controls.addEventListener("change", onControlsChange);
    const animate = () => {
      frame = window.requestAnimationFrame(animate);
      const frameNow = performance.now();
      const activeParticles = eventsRef.current.some((event) => nowRef.current - event.observedAt < 4_500);
      const activeScene = autoRotateRef.current || activeParticles || frameNow < interactionUntil;
      const targetFps = activeScene ? profile.activeFps : profile.idleFps;
      if (frameNow - lastFrameAt < 1000 / targetFps) return;
      lastFrameAt = frameNow;
      const elapsed = frameNow / 1000;
      const currentSelectedNodeId = selectedNodeIdRef.current;
      if (frameNow - lastParticleSyncAt >= 120) {
        lastParticleSyncAt = frameNow;
        ensureParticles();
      }
      stars.rotation.y = elapsed * 0.012;
      edgeGroup.visible = routesVisibleRef.current;
      controls.autoRotate = autoRotateRef.current;
      planets.forEach(({ node, mesh, baseScale }) => {
        mesh.rotation.y += node.registered ? 0.006 : 0.003;
        const selected = node.id === currentSelectedNodeId;
        const pulse = selected ? 1.24 + Math.sin(elapsed * 4) * 0.04 : 1;
        mesh.scale.setScalar(baseScale * pulse);
        if (selected) {
          selectedRing.visible = true;
          selectedRing.position.copy(mesh.position);
          selectedRing.scale.setScalar(baseScale * 1.6);
          selectedRing.rotation.x = Math.PI / 2 + elapsed * 0.35;
          selectedRing.rotation.z = elapsed * 0.55;
        }
      });
      if (!currentSelectedNodeId) selectedRing.visible = false;
      const wallNow = Date.now();
      particles.forEach((particle) => {
        const age = (wallNow - particle.observedAt) / 1400;
        const progress = ((age % 1) + 1) % 1;
        particle.mesh.position.copy(particle.source).lerp(particle.target, progress);
        particle.mesh.visible = wallNow - particle.observedAt < 6_000;
      });
      controls.update();
      cameraStateRef.current = {
        position: camera.position.clone(),
        target: controls.target.clone(),
      };
      renderer.render(scene, camera);
    };
    animate();

    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", onResize);
      renderer.domElement.removeEventListener("pointerdown", onPointerDown);
      renderer.domElement.removeEventListener("pointermove", onPointerMove);
      renderer.domElement.removeEventListener("pointerup", onPointerUp);
      controls.removeEventListener("change", onControlsChange);
      controls.dispose();
      renderer.dispose();
      scene.traverse((object) => {
        if (object instanceof THREE.Mesh || object instanceof THREE.Line || object instanceof THREE.Points) {
          object.geometry?.dispose();
          const materials = Array.isArray(object.material) ? object.material : [object.material];
          materials.forEach((material) => material.dispose());
        }
      });
      mount.replaceChildren();
    };
  }, [sceneData, onInspectNode]);

  const selectedNode = nodes.find((node) => node.id === selectedNodeId);

  return (
    <div className="space-explorer" role="dialog" aria-modal="true" aria-label="トポロジー宇宙探索ビュー">
      <div className="space-explorer__stage" ref={mountRef}>
        {fallback && <div className="space-explorer__fallback">{fallback}</div>}
      </div>
      <div className="space-explorer__hud">
        <div>
          <strong>宇宙探索</strong>
          <span>{nodes.length} 惑星 · {edges.length} 航路</span>
        </div>
        <div className="space-explorer__controls" aria-label="宇宙探索表示設定">
          <button
            className={`space-explorer__toggle${routesVisible ? " space-explorer__toggle--active" : ""}`}
            aria-pressed={routesVisible}
            onClick={() => setRoutesVisible((value) => !value)}
          >
            <Route size={15} />航路
          </button>
          <button
            className={`space-explorer__toggle${autoRotate ? " space-explorer__toggle--active" : ""}`}
            aria-pressed={autoRotate}
            onClick={() => setAutoRotate((value) => !value)}
          >
            <RotateCcw size={15} />自動回転
          </button>
          <button className="icon-button space-explorer__close" aria-label="宇宙探索を閉じる" title="閉じる" onClick={onClose}><X size={18} /></button>
        </div>
      </div>
      <section className="space-explorer__events" aria-label="観測イベントログ">
        <h3>観測イベント</h3>
        <div>
          {visibleEvents.length === 0 ? <p>イベントを待っています</p> : visibleEvents.map((event) => {
            const targetNodeId = event.harvestedNodeId ?? event.nodeId;
            const node = eventNodes.get(targetNodeId);
            return (
              <article className={`space-explorer__event space-explorer__event--${event.kind}`} key={event.id}>
                <i />
                <div>
                  <strong>{eventLabels[event.kind]}</strong>
                  <span>{node?.label ?? targetNodeId}</span>
                  <small>{event.height ? `高さ ${event.height} · ` : ""}{new Date(event.observedAt).toLocaleTimeString("ja-JP")}</small>
                </div>
              </article>
            );
          })}
        </div>
      </section>
      <div className="space-explorer__status" aria-live="polite">
        <span>{selectedNode ? selectedNode.label : "惑星をクリックしてノードを選択"}</span>
        <small>ドラッグで回転 · ホイールでズーム · 右ドラッグで移動</small>
      </div>
    </div>
  );
}
