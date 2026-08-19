/**
 * Creature / entity presets for sculpt.html?preset=<id>
 * Each preset defines skeleton parts, labels, voxel seed and walk behaviour.
 * Pivots are plain {x,y,z}; sculpt.html converts them to THREE.Vector3.
 */

function V(x, y, z) {
    return { x, y, z };
}

function emptyParts(names) {
    const o = {};
    for (const n of names) o[n] = [];
    return o;
}

function addVoxel(arr, x, y, z, c) {
    const ix = Math.round(x), iy = Math.round(y), iz = Math.round(z);
    const hit = arr.find((v) => v.x === ix && v.y === iy && v.z === iz);
    if (hit) hit.c = c;
    else arr.push({ x: ix, y: iy, z: iz, c });
}

function box(arr, x0, x1, y0, y1, z0, z1, c) {
    for (let x = x0; x <= x1; x++) {
        for (let y = y0; y <= y1; y++) {
            for (let z = z0; z <= z1; z++) addVoxel(arr, x, y, z, c);
        }
    }
}

/** Classic biped hero (current editor). */
const biped = {
    id: 'biped',
    label: 'Bípedo humano',
    blurb: '2 brazos, 2 piernas, pies planos. Personaje jugable.',
    kind: 'character',
    defaultName: 'hero',
    grid: { x: 16, y: 32, z: 16 },
    hasWalk: true,
    partOrder: [
        'torso', 'head',
        'lArm', 'lForearm', 'rArm', 'rForearm',
        'lLeg', 'lShin', 'rLeg', 'rShin',
        'lFoot', 'rFoot',
    ],
    partLabels: {
        torso: 'Torso', head: 'Cabeza',
        lArm: 'Brazo Izq', lForearm: 'Antebrazo Izq',
        rArm: 'Brazo Der', rForearm: 'Antebrazo Der',
        lLeg: 'Muslo Izq', lShin: 'Pierna Izq',
        rLeg: 'Muslo Der', rShin: 'Pierna Der',
        lFoot: 'Pie Izq', rFoot: 'Pie Der',
    },
    parts: {
        torso: { pivot: V(0, 20, 0), parent: null },
        head: { pivot: V(0, 26, 0), parent: 'torso' },
        lArm: { pivot: V(-6.5, 23, 0), parent: 'torso' },
        lForearm: { pivot: V(-6.5, 19.5, 0), parent: 'lArm' },
        rArm: { pivot: V(6.5, 23, 0), parent: 'torso' },
        rForearm: { pivot: V(6.5, 19.5, 0), parent: 'rArm' },
        lLeg: { pivot: V(-2.5, 15, 0), parent: 'torso' },
        lShin: { pivot: V(-2.5, 10, 0), parent: 'lLeg' },
        rLeg: { pivot: V(2.5, 15, 0), parent: 'torso' },
        rShin: { pivot: V(2.5, 10, 0), parent: 'rLeg' },
        lFoot: { pivot: V(-2.5, 5, 0), parent: 'lShin' },
        rFoot: { pivot: V(2.5, 5, 0), parent: 'rShin' },
    },
    jointKeys: [
        'lArmX', 'lForearmX', 'rArmX', 'rForearmX',
        'lLegX', 'lShinX', 'rLegX', 'rShinX',
        'lFootX', 'rFootX',
    ],
    generateVoxels(C) {
        const d = emptyParts(this.partOrder);
        box(d.torso, -5, 5, -4, 4, -3, 3, C.SHIRT);
        box(d.torso, -5, 5, -4, -4, -3, 3, C.BOOTS);
        addVoxel(d.torso, 0, -4, -4, C.GOLD);
        box(d.torso, -4, 4, -2, 2, 4, 4, C.CAPE);
        box(d.torso, 6, 6, 2, 4, -3, 3, C.METAL);
        box(d.torso, -6, -6, 2, 4, -3, 3, C.METAL);
        for (let x = -5; x <= 5; x++) {
            for (let y = -4; y <= 4; y++) {
                for (let z = -5; z <= 5; z++) {
                    const dist = Math.sqrt(x * x + y * y + z * z);
                    if (dist <= 4.5) addVoxel(d.head, x, y, z, C.SKIN);
                    if (dist <= 5.5 && y > 1) addVoxel(d.head, x, y, z, C.METAL);
                }
            }
        }
        addVoxel(d.head, 2, 1, -4, C.EYE);
        addVoxel(d.head, -2, 1, -4, C.EYE);
        addVoxel(d.head, 0, -2, -5, C.MOUTH);
        // Upper arm (shoulder → elbow) + forearm (elbow → hand).
        box(d.lArm, -1, 1, -3, 1, -2, 2, C.SHIRT);
        box(d.lForearm, -1, 1, -4, 0, -2, 2, C.SKIN);
        box(d.rArm, -1, 1, -3, 1, -2, 2, C.SHIRT);
        box(d.rForearm, -1, 1, -4, 0, -2, 2, C.SKIN);
        // Thigh + shin + foot.
        box(d.lLeg, -1, 1, -4, 0, -2, 2, C.PANTS);
        box(d.lShin, -1, 1, -4, 0, -2, 2, C.PANTS);
        box(d.rLeg, -1, 1, -4, 0, -2, 2, C.PANTS);
        box(d.rShin, -1, 1, -4, 0, -2, 2, C.PANTS);
        box(d.lFoot, -1, 1, -1, 0, -2, 2, C.BOOTS);
        box(d.rFoot, -1, 1, -1, 0, -2, 2, C.BOOTS);
        return d;
    },
    walk(j, t) {
        const s = Math.sin(t * 4.5);
        const c = Math.cos(t * 4.5);
        const leg = s * 28;
        const arm = -s * 22;
        j.lLegX = leg; j.rLegX = -leg;
        j.lArmX = arm; j.rArmX = -arm;
        // Knee flex backward (deg); negative X = shin swings toward −Z / behind.
        j.lShinX = -Math.max(0, -s) * 40;
        j.rShinX = -Math.max(0, s) * 40;
        j.lForearmX = 18 + Math.abs(s) * 16;
        j.rForearmX = 18 + Math.abs(s) * 16;
        j.lFootX = c * 12 * 0.45;
        j.rFootX = -c * 12 * 0.45;
    },
};

/** Biped with goat / satyr hooves. */
const satyr = {
    ...biped,
    id: 'satyr',
    label: 'Bípedo pezuña',
    blurb: 'Como el bípedo, pero pies de cabra / sátiro.',
    defaultName: 'satyr',
    generateVoxels(C) {
        const d = biped.generateVoxels.call(this, C);
        d.lFoot = [];
        d.rFoot = [];
        // Hoof wedge pointing forward (−Z).
        box(d.lFoot, -1, 1, -1, 0, -1, 1, C.BOOTS);
        addVoxel(d.lFoot, 0, -1, -2, C.BOOTS);
        addVoxel(d.lFoot, 0, 0, -2, C.GOLD);
        box(d.rFoot, -1, 1, -1, 0, -1, 1, C.BOOTS);
        addVoxel(d.rFoot, 0, -1, -2, C.BOOTS);
        addVoxel(d.rFoot, 0, 0, -2, C.GOLD);
        return d;
    },
};

/** Dog / cat style quadruped. */
const quadruped = {
    id: 'quadruped',
    label: 'Cuadrúpedo',
    blurb: 'Perro, gato, lobo… cuerpo horizontal y 4 patas.',
    kind: 'character',
    defaultName: 'beast',
    grid: { x: 16, y: 16, z: 24 },
    hasWalk: true,
    partOrder: ['torso', 'head', 'tail', 'flLeg', 'frLeg', 'blLeg', 'brLeg'],
    partLabels: {
        torso: 'Cuerpo', head: 'Cabeza', tail: 'Cola',
        flLeg: 'Pata DI', frLeg: 'Pata DD', blLeg: 'Pata TI', brLeg: 'Pata TD',
    },
    parts: {
        torso: { pivot: V(0, 8, 0), parent: null },
        head: { pivot: V(0, 10, -7), parent: 'torso' },
        tail: { pivot: V(0, 9, 6), parent: 'torso' },
        flLeg: { pivot: V(-3, 6, -4), parent: 'torso' },
        frLeg: { pivot: V(3, 6, -4), parent: 'torso' },
        blLeg: { pivot: V(-3, 6, 4), parent: 'torso' },
        brLeg: { pivot: V(3, 6, 4), parent: 'torso' },
    },
    jointKeys: ['headY', 'flLegX', 'frLegX', 'blLegX', 'brLegX', 'tailY'],
    generateVoxels(C) {
        const d = emptyParts(this.partOrder);
        box(d.torso, -3, 3, -2, 2, -5, 5, C.SHIRT);
        box(d.head, -2, 2, -2, 2, -3, 1, C.SKIN);
        addVoxel(d.head, -1, 1, -3, C.EYE);
        addVoxel(d.head, 1, 1, -3, C.EYE);
        box(d.tail, -1, 1, -1, 1, 0, 4, C.PANTS);
        for (const leg of ['flLeg', 'frLeg', 'blLeg', 'brLeg']) {
            box(d[leg], -1, 1, -6, 0, -1, 1, C.BOOTS);
        }
        return d;
    },
    walk(j, t) {
        const a = Math.sin(t * 5) * 28;
        j.flLegX = a; j.brLegX = a;
        j.frLegX = -a; j.blLegX = -a;
        j.tailY = Math.sin(t * 3) * 20;
    },
};

/** Spider / arachnid — 8 legs. */
const arachnid = {
    id: 'arachnid',
    label: 'Arácnido',
    blurb: 'Araña u otro arácnido: cefalotórax, abdomen y 8 patas.',
    kind: 'character',
    defaultName: 'spider',
    grid: { x: 24, y: 12, z: 24 },
    hasWalk: true,
    partOrder: [
        'torso', 'abdomen',
        'leg1', 'leg2', 'leg3', 'leg4',
        'leg5', 'leg6', 'leg7', 'leg8',
    ],
    partLabels: {
        torso: 'Cefalotórax', abdomen: 'Abdomen',
        leg1: 'Pata 1', leg2: 'Pata 2', leg3: 'Pata 3', leg4: 'Pata 4',
        leg5: 'Pata 5', leg6: 'Pata 6', leg7: 'Pata 7', leg8: 'Pata 8',
    },
    parts: {
        torso: { pivot: V(0, 4, -2), parent: null },
        abdomen: { pivot: V(0, 4, 3), parent: 'torso' },
        leg1: { pivot: V(-3, 4, -3), parent: 'torso' },
        leg2: { pivot: V(-4, 4, -1), parent: 'torso' },
        leg3: { pivot: V(-4, 4, 1), parent: 'torso' },
        leg4: { pivot: V(-3, 4, 3), parent: 'torso' },
        leg5: { pivot: V(3, 4, -3), parent: 'torso' },
        leg6: { pivot: V(4, 4, -1), parent: 'torso' },
        leg7: { pivot: V(4, 4, 1), parent: 'torso' },
        leg8: { pivot: V(3, 4, 3), parent: 'torso' },
    },
    jointKeys: ['leg1Z', 'leg2Z', 'leg3Z', 'leg4Z', 'leg5Z', 'leg6Z', 'leg7Z', 'leg8Z'],
    generateVoxels(C) {
        const d = emptyParts(this.partOrder);
        box(d.torso, -2, 2, -1, 1, -2, 2, C.PANTS);
        box(d.abdomen, -3, 3, -2, 2, -1, 3, C.SHIRT);
        for (let i = 1; i <= 8; i++) {
            box(d[`leg${i}`], -1, 1, -1, 1, 0, 5, C.BOOTS);
        }
        return d;
    },
    walk(j, t) {
        for (let i = 1; i <= 8; i++) {
            const phase = (i % 2 === 0) ? 0 : Math.PI;
            j[`leg${i}Z`] = Math.sin(t * 6 + phase) * 25;
        }
    },
};

/** Fish — body + tail + fins, swim instead of walk. */
const fish = {
    id: 'fish',
    label: 'Pez',
    blurb: 'Cuerpo, cola y aletas. Animación de nado.',
    kind: 'character',
    defaultName: 'fish',
    grid: { x: 12, y: 12, z: 24 },
    hasWalk: true,
    partOrder: ['torso', 'head', 'tail', 'finL', 'finR', 'finTop'],
    partLabels: {
        torso: 'Cuerpo', head: 'Cabeza', tail: 'Cola',
        finL: 'Aleta Izq', finR: 'Aleta Der', finTop: 'Aleta dorsal',
    },
    parts: {
        torso: { pivot: V(0, 6, 0), parent: null },
        head: { pivot: V(0, 6, -5), parent: 'torso' },
        tail: { pivot: V(0, 6, 5), parent: 'torso' },
        finL: { pivot: V(-2, 6, 0), parent: 'torso' },
        finR: { pivot: V(2, 6, 0), parent: 'torso' },
        finTop: { pivot: V(0, 8, 0), parent: 'torso' },
    },
    jointKeys: ['tailY', 'finLZ', 'finRZ', 'headY'],
    generateVoxels(C) {
        const d = emptyParts(this.partOrder);
        box(d.torso, -2, 2, -2, 2, -4, 4, C.SHIRT);
        box(d.head, -2, 2, -2, 2, -2, 1, C.SKIN);
        addVoxel(d.head, -1, 1, -2, C.EYE);
        addVoxel(d.head, 1, 1, -2, C.EYE);
        box(d.tail, -1, 1, -2, 2, 0, 3, C.CAPE);
        box(d.finL, -3, 0, 0, 0, -1, 1, C.METAL);
        box(d.finR, 0, 3, 0, 0, -1, 1, C.METAL);
        box(d.finTop, 0, 0, 0, 2, -1, 1, C.GOLD);
        return d;
    },
    walk(j, t) {
        j.tailY = Math.sin(t * 6) * 35;
        j.finLZ = Math.sin(t * 5) * 20;
        j.finRZ = -Math.sin(t * 5) * 20;
        j.headY = Math.sin(t * 2) * 8;
    },
};

/** Jellyfish — bell + tentacles. */
const jellyfish = {
    id: 'jellyfish',
    label: 'Medusa',
    blurb: 'Campana y tentáculos colgantes. Pulso suave.',
    kind: 'character',
    defaultName: 'jellyfish',
    grid: { x: 16, y: 24, z: 16 },
    hasWalk: true,
    partOrder: ['torso', 'tent1', 'tent2', 'tent3', 'tent4', 'tent5', 'tent6'],
    partLabels: {
        torso: 'Campana',
        tent1: 'Tent. 1', tent2: 'Tent. 2', tent3: 'Tent. 3',
        tent4: 'Tent. 4', tent5: 'Tent. 5', tent6: 'Tent. 6',
    },
    parts: {
        torso: { pivot: V(0, 16, 0), parent: null },
        tent1: { pivot: V(-2, 14, -2), parent: 'torso' },
        tent2: { pivot: V(2, 14, -2), parent: 'torso' },
        tent3: { pivot: V(-3, 14, 0), parent: 'torso' },
        tent4: { pivot: V(3, 14, 0), parent: 'torso' },
        tent5: { pivot: V(-2, 14, 2), parent: 'torso' },
        tent6: { pivot: V(2, 14, 2), parent: 'torso' },
    },
    jointKeys: ['tent1X', 'tent2X', 'tent3X', 'tent4X', 'tent5X', 'tent6X'],
    generateVoxels(C) {
        const d = emptyParts(this.partOrder);
        for (let x = -4; x <= 4; x++) {
            for (let y = -2; y <= 2; y++) {
                for (let z = -4; z <= 4; z++) {
                    if (x * x + z * z <= 18 && y >= -1) addVoxel(d.torso, x, y, z, C.SHIRT);
                }
            }
        }
        for (let i = 1; i <= 6; i++) {
            box(d[`tent${i}`], 0, 0, -10, 0, 0, 0, C.SKIN);
            box(d[`tent${i}`], 0, 0, -10, -4, 0, 0, C.CAPE);
        }
        return d;
    },
    walk(j, t) {
        for (let i = 1; i <= 6; i++) {
            j[`tent${i}X`] = Math.sin(t * 2.2 + i * 0.7) * 18;
        }
    },
};

/** Flat prop / furniture — no skeleton. */
const object = {
    id: 'object',
    label: 'Objeto',
    blurb: 'Prop sin rotación de articulaciones. Tamaño libre.',
    kind: 'object',
    defaultName: 'object',
    grid: { x: 16, y: 16, z: 16 },
    hasWalk: false,
    partOrder: [],
    partLabels: {},
    parts: {},
    jointKeys: [],
    generateVoxels() { return {}; },
    walk() {},
};

export const PRESETS = {
    biped,
    satyr,
    quadruped,
    arachnid,
    fish,
    jellyfish,
    object,
};

export const PRESET_LIST = [
    biped, satyr, quadruped, arachnid, fish, jellyfish, object,
];

export function getPreset(id) {
    return PRESETS[id] || biped;
}
