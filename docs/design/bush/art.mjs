// #462 bush theme, design mockup: the animals by role, the moments, the hollow-log rail, the outback clearing with its
// billabong and Fire Danger Rating sign, the sign minis, the row sprites, the icon and the ASCII figures all come from
// the pixel maps below (one character = one pixel), like ../kitchen/art.mjs and #423's garden. index.html and sheet.html
// import this module; phase 2's build writes the theme files (sprites.svg, scene.svg, icons) from the same maps.
// Lane sprites keep the farm's box: 132 × 100, one pixel = 4 units (a 33 × 25 grid), feet on y 23, shadow on y 24.
// Row sprites are a 16 × 16 grid, one pixel = 1 (drawn at 24 px, 1.5×). Per-lane fur colours are CSS custom properties
// (`looks`: one set covers all four animals, so a lane keeps its look across roles); `--fk` is the repo colour, worn as
// a drover's neckerchief.

// One character → a fill (and an opacity). `.` and space are transparent.
export const INK = {
  k: '#241a14', q: ['#241a14', 0.18], Q: '#ffffff', x: ['#ffffff', 0.7], T: 'var(--fk,#58a6ff)',
  // kangaroo (and joey): fur, its shade, the pale belly
  R: 'var(--ro,#b8643a)', r: 'var(--rs,#8c4527)', c: 'var(--rb,#ecd2aa)',
  // wombat: fur, shade, the broad nose
  W: 'var(--wo,#7d6a58)', w: 'var(--ws,#5b4b3e)', n: '#2e2420',
  // koala: fur, shade, ear fluff and chest, the big nose
  G: 'var(--ko,#9ea3a8)', g: 'var(--ks,#71777d)', E: '#efece6', N: '#2a2522',
  // emu: shaggy feathers, their shade, the blue neck skin, legs
  M: 'var(--em,#5c4a38)', m: 'var(--es,#3f3226)', B: '#7b9cc0', Y: '#8f7d5a',
  // kookaburra: brown back, its shade, cream head, the blue wing patch, beak (dark top, pale bottom), rufous tail
  U: '#7a5232', u: '#553820', F: '#efe6d2', V: '#5f93c4', X: '#3a3632', Z: '#cdbd9c', J: '#b8602e',
  // cockatoo: the sulphur crest; its white is Q with an f edge; grey beak and feet
  H: '#f2d23a', h: '#d6b126', f: '#b9b09e', i: '#5f6670',
  // ghost gum: trunk, trunk shade, gum-leaf sage, its shade
  P: '#e6dfcf', p: '#b6ab94', L: '#7d9a66', l: '#5b7a4c',
  // red earth, its shade, dust; the hollow log: wood, shade, the dark inside, highlight; the emu egg
  D: '#b5562e', d: '#8a3d1f', e: '#dc9a66', O: '#8b6a48', o: '#62482f', A: '#2f2117', a: '#ad8a62', K: '#2f6a5c', j: '#5f9a88',
};

// ---- map helpers ----
const flipH = m => m.map(r => [...r].reverse().join(''));
const recolor = (m, tr) => m.map(r => [...r].map(c => tr[c] || c).join(''));
// A pixel line (Bresenham) as layers of one-pixel maps.
function line(x0, y0, x1, y1, ch) {
  const L = [], dx = Math.abs(x1 - x0), dy = -Math.abs(y1 - y0), sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
  let err = dx + dy;
  for (;;) {
    L.push([[ch], x0, y0]);
    if (x0 === x1 && y0 === y1) break;
    const e2 = 2 * err;
    if (e2 >= dy) { err += dy; x0 += sx; }
    if (e2 <= dx) { err += dx; y0 += sy; }
  }
  return L;
}
const f = (l, k) => l.map(a => [a[0], a[1], a[2], k]);
const at = (l, dx, dy) => l.map(a => [a[0], a[1] + dx, a[2] + dy, a[3]]);

// ---- the animals (lane scale) ----
// impl: a kangaroo bounding. Frame 1 crouched on its big feet, frame 2 in the air, legs out behind.
const ROO_CROUCH = [
  '................r..r.....',
  '................Rr.Rr....',
  '................RRRRR....',
  '...............RRkRRRR...',
  '...............RRRRRRRRRk',
  '................RRRRRRR..',
  '...............TTTTTc....',
  '.............RRRRTTcc....',
  '............RRRRRRccc....',
  '...........RRRRRRRcccR...',
  '..........RRRRRRRRcccRk..',
  '.........RRRRRRRRRccc....',
  '........rRRRRRRRRRcc.....',
  '.......rr.RRRRRRRRRc.....',
  '......rr..rRRRRRRRRR.....',
  '.....rr...rrRRRRRRRR.....',
  '....rr....rrrRRRRRRr.....',
  '...rr......rrrRRRRr......',
  '..rr........rrrrr........',
  '.rr.........rrrrrrrrrk...',
  'rrr........rrrrrrrrrrr...',
];
const ROO_AIR = [
  '.....................r.r....',
  '.....................RrRr...',
  '....................RRRRR...',
  '...................RRkRRRR..',
  '...................RRRRRRRRk',
  '.................TTTTRRRR...',
  'rr.........RRRRRRTTTcc......',
  '.rrr....RRRRRRRRRRRccc......',
  '...rrrRRRRRRRRRRRRRcccRR....',
  '.....rRRRRRRRRRRRRRccc..k...',
  '......RRRRRRRRRRRRRcc.......',
  '.......rRRRRRRRRRRc.........',
  '......rrrrRRRRRRr...........',
  '....rrrrrrrrrr..............',
  '..rrrrrr....................',
  'rrrrk.......................',
];
// fix: a wombat digging into a mound, dirt flying out behind on two frames.
const WOMBAT = [
  '.....wwwwwwwww.........',
  '...wwWWWWWWWWWWww......',
  '..wWWWWWWWWWWWWWWw.ww..',
  '.wWWWWWWWWWWWWWWWWwWWw.',
  '.wWWWWWWWWWWWWWWWTWWWW.',
  'wWWWWWWWWWWWWWWWWTWkWW.',
  'wWWWWWWWWWWWWWWWWTWWWWn',
  'wwWWWWWWWWWWWWWWWWWWWnn',
  '.wwWWWWWWWWWWWWWWWWWWW.',
];
const WLEGS1 = ['..wwW....wwW....wwW.wW.', '..ww.....ww.....w...ww.'];
const WLEGS2 = ['..wwW...wwW......wwWwW.', '.ww......w.......ww.ww.'];
const MOUND = ['....eDDD..', '..eDDDDDD.', '.eDDAADDDD', 'eDDAAAADDd', 'DDAAAAADDd', 'DDDAAADDdd', 'dDDDDDDddd'];
// rev: a koala on a gum branch, peering down over its end. Its head dips a pixel on frame 2.
const GUM = (x, top, bot) => [[Array(bot - top).fill('PPPp'), x, top], [['..lLLl', 'lLLLLLLl', 'LLlLLLLLl', '.lLLLlLL'], x - 3, top - 1]];
const KBODY = ['....gGGGGGg..', '..gGGGGGGGGGg', '.gGGGGGGGGGGG', 'gGGGGGGGGGGGG', 'gGGGGGGGGGGGg', 'gGGGGGGGGGGg.', '.gggGGGGGgg..'];
const KHEAD = ['EEE.....EEE', 'EGGE...EGGE', 'EGGGGGGGGGE', '.GGGGGGGGG.', '.GGGGGGGGG.', '.GkGNNNGkG.', '..GGNNNGG..', '...GNNNG...', '....GGG....'];
const KHEAD_SHUT = ['EEE.....EEE', 'EGGE...EGGE', 'EGGGGGGGGGE', '.GGGGGGGGG.', '.GkkGGGkkG.', '.GGGNNNGGG.', '..GGNNNGG..', '...GNNNG...', '....GGG....'];
const KHEAD_BIG = ['EEE.......EEE', 'EGGE.....EGGE', 'EGGGGGGGGGGGE', '.GGGGGGGGGGG.', '.GGGGGGGGGGG.', '.GGkGGGGGkGG.', '.GGGGNNNGGGG.', '..GGGNNNGGG..', '...GTNNNTG...', '....TTTTT....'];
const KSIT = ['..gGGGGGGg.', '.gGGEEEGGGg', 'gGGGEEEGGGG', 'gGGGGGGGGGg', '.gGGGGGGGg.'];
// update: an emu running, legs scissoring on the two frames.
const EMU = [
  '....mMMMMMMm.....',
  '..mMMMMMMMMMMm...',
  '.mMMmMMMmMMMMMm..',
  'mMMMMMMMMMMMMMMM.',
  'mMmMMMmMMMMmMMMMm',
  '.mMMMMMMMMMMMMMm.',
  '..mMmMMMMmMMMm...',
  '....mmMMMMmm.....',
];
const EMU_NECK = ['....kMM.', '...MMMMX', '...MkMM.', '...BBB..', '..BBM...', '..BTB...', '.BBM....', '.BTB....', 'MMB.....'];
// done: a kookaburra laughing on a fence post, its beak open on frame 2.
const KOOK = [
  '....FFFFFF.......',
  '...FFuuuuFF......',
  '..FFFFFFFFFF.....',
  '..FFFFkFuuuFXXXX.',
  '..FFFFFFFFFFZZZZX',
  '...FFFFFFFFF.....',
  '..UUUFFFFFF......',
  '.UUVUUTTTFF......',
  '.UVUUUUFFFF......',
  'UUUVUUUUFFF......',
  'UUUUUUUUFF.......',
  'JUUUUUUUF........',
  'JJ.UUUUu.........',
  'JJ..Y..Y.........',
];
const KOOK_OPEN = ['..........XX.....', '....FFFFFFXXX....', '...FFuuuuFXX.....', '..FFFFFFFFF......', '..FFFFkFuuuF.....', '..FFFFFFFFFZZ....', '...FFFFFFFF.ZZZ..'];
const POST = ['oOOOOo', 'aOOOOo', '.OOOO.', '.OOOo.', '.OOOo.', '.OOOo.', '.OOOo.', '.OOOo.', '.OOOo.', '.OOOo.'];
const HA = ['Q.Q.QQ', 'QQQ.QQ', 'Q.QQ.Q'];

// The 7 actions (the contract's `actions`): f1/f2 are the loop's two frames.
const RSHADOW = ['..qqqqqqqqqqqqqqqqqqqq..'];
export const ACTIONS = {
  bound: () => [[RSHADOW, 3, 24], [ROO_CROUCH, 4, 3, 'f1'], [['qqqqqqqqqqqq'], 9, 24, 'f2'], [ROO_AIR, 3, 1, 'f2'],
    [['e.e', '.e.'], 2, 22, 'f2']],
  dig: () => [[['.qqqqqqqqqqqqqqqqqqqqqqqqqq.'], 1, 24], [MOUND, 23, 17], [WOMBAT, 2, 14], [WLEGS1, 2, 22, 'f1'], [WLEGS2, 2, 22, 'f2'],
    [['d.', '.e', '..', 'd.'], 0, 12, 'f1'], [['e', '.', '.', 'd'], 3, 9, 'f1'], [['.d', 'e.'], 1, 17, 'f2'], [['d', '.', 'e'], 0, 8, 'f2'], [['d'], 4, 13, 'f2']],
  peer: () => [...GUM(25, 2, 25), [Array(2).fill('ppPPPPPPPPPPPPPPPPPPP'), 4, 15], [['.l', 'lL'], 3, 17], [['qqqqqqq'], 22, 24],
    [KSIT, 8, 11], [['gg', 'gg'], 8, 14], [['gg', 'gg'], 17, 14], [['gg', 'g.'], 10, 17], [['gg', '.g'], 15, 17],
    [KHEAD_BIG, 7, 2, 'f1'], [KHEAD_BIG, 6, 3, 'f2']],
  run: () => [[['..qqqqqqqqqqqqqqqqq..'], 4, 24], [EMU, 3, 7], [EMU_NECK, 15, 0], [['MM', 'mm'], 13, 14],
    ...f([...line(10, 15, 14, 19, 'Y'), ...line(14, 19, 18, 23, 'Y'), [['YYY'], 18, 23], ...line(9, 15, 6, 19, 'Y'), ...line(6, 19, 2, 21, 'Y'), [['YY'], 1, 21]], 'f1'),
    ...f([...line(10, 15, 10, 19, 'Y'), ...line(10, 19, 12, 23, 'Y'), [['YYY'], 12, 23], ...line(9, 15, 7, 19, 'Y'), ...line(7, 19, 9, 22, 'Y'), [['YY'], 9, 22]], 'f2')],
  sleep: () => [[['qqqqqqqqqqqq'], 10, 24], [Array(11).fill('PPPPp'), 13, 14], ...line(13, 14, 6, 4, 'P'), ...line(14, 14, 7, 4, 'P'), ...line(17, 14, 24, 3, 'p'), ...line(16, 14, 23, 3, 'P'),
    [['.lLLl.', 'lLLLLLl', 'LlLLLL.', '.LLl'], 3, 1], [['.lLLl.', 'lLLLLLl', '.LLlLLL', '..lLL'], 21, 0],
    [['...gGGGGGGGg...', '.gGGGGGGGGGGGg.', 'gGGGGGGGGGGGGGg', '.gGGGGGGGGGGGg.', '...gggggggggg..'], 8, 9], [KHEAD_SHUT, 6, 4],
    [['QQ', '.Q', 'QQ'], 19, 2, 'f1'], [['QQQ', '..Q', '.Q.', 'QQQ'], 22, 0, 'f2']],
  laugh: () => [[['qqqqqqqq'], 11, 24], [POST, 11, 15], [KOOK, 7, 1, 'f1'], [KOOK.slice(7), 7, 8, 'f2'], [KOOK_OPEN, 7, 1, 'f2'], [HA, 25, 1, 'f2'], [HA, 26, 5, 'f2']],
  stuck: () => [[['.qqqqqqqqqqqqqqqqqqqqqqqq.'], 3, 24],
    [['........eeDDDDDDDD........', '.....eDDDDDDDDDDDDDDDe.....', '...eDDDDDDDDDDDDDDDDDDDd...', '..eDDDDDDAAAAAAAAADDDDDDd..', '.eDDDDDAAAAAAAAAAAAADDDDDd.',
      'eDDDDDAAAAAAAAAAAAAAAADDDDd', 'DDDDDAAAAAAAAAAAAAAAAAADDdd', 'DDDDDAAAAAAAAAAAAAAAAAADDdd', 'DDDDDAAAAAAAAAAAAAAAAAADddd', 'dDDDDAAAAAAAAAAAAAAAAAADddd'], 3, 14],
    [['....wwwwwwwww....', '..wWWWWWWWWWWWw..', '.wWWWWWWWWWWWWWw.', 'wWWWWWWWwWWWWWWWw', 'wWWWWWWWwWWWWWWWw', 'wWWWWWWWWWWWWWWWw', 'wwWWWWWWWWWWWWWww'], 8, 16],
    [['ww', 'Ww', 'ww'], 7, 20, 'f1'], [['ww', 'wW', 'ww'], 24, 21, 'f1'], [['ww', 'Ww', 'ww'], 7, 21, 'f2'], [['ww', 'wW', 'ww'], 24, 20, 'f2'], [['w'], 16, 15],
    [['e.e', '.e.'], 2, 12, 'f1'], [['.e.', 'e.e'], 28, 11, 'f2']],
};
// The contract's `actions`, by role and state: the sprite id is `a-<action>`.
export const ACT_OF = { impl: 'bound', fix: 'dig', rev: 'peer', update: 'run', idle: 'sleep', done: 'laugh', blocked: 'stuck' };
export const ACT_WORD = { bound: 'bound', dig: 'dig', peer: 'peer', run: 'run', sleep: 'asleep', laugh: 'laugh', stuck: 'stuck' };

// ---- the moments (#241 `moments`) and the rail ----
const MAGPIE = ['kk..........', '.kkk.....kQ.', '..kkkQQ.kkkki', '...QQkkkkkk.', '....kkkQkk..', '......kk....'];
const MAGPIE_DOWN = ['..........kQ.', 'kk.....kkkkki', '.kkkQQkkkkk..', '..QQkkkkQ....', '...kkkk......'];
const COCKY = ['..H.H.....', '..HHH.H...', '...HHHH...', '..fQQQQf..', '.fQQQkQQf.', '.fQQQQQii.', '.fQQQQQi..', 'fQQQQQQf..', 'fQQfQQQf..', 'fQQQQQf...', '.fQQQQf...', '..fQf.....', '..i.i.....'];
const COCKY_OPEN = ['.H..H.H...', '..H.HH.H..', '..HHHHH...', '..fQQQQf..', '.fQQQkQQii', '.fQQQQQf..', '.fQQQQQii.'];
const SCREECH = ['..Q', '.Q.', '...', 'QQ.', '...', '.Q.', '..Q'];
// The hollow log: a section with a knot hole, so the rail's items in a row read as one log (`hole` draws what's in it).
function logPart(y0 = 10) {
  return [[['aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', ...Array(12).fill('OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO'), 'ooooooooooooooooooooooooooooooooo', 'qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq'], 0, y0],
    [['oo....oo...oooo....ooo....oo..oo'], 0, y0 + 3], [['....ooo....o.....ooo....oooo....'], 0, y0 + 9],
    [['...oooooo...', '.ooAAAAAAoo.', 'oAAAAAAAAAAo', 'oAAAAAAAAAAo', 'oAAAAAAAAAAo', '.ooAAAAAAoo.', '...oooooo...'], 10, y0 + 4]];
}
const JOEY = ['.r..r.', '.Rr.Rr', '.RRRR.', 'RkRRkR', '.RRRRk'];
const EGG = ['..KKK..', '.KjKKK.', 'KjKKKKK', 'KKKKKKK', '.KKKKK.'];
export const RAIL = {
  joey: () => [...logPart(), [JOEY, 13, 11, 'f1'], [JOEY, 13, 12, 'f2']],
  egg: () => [...logPart(), [EGG, 13, 16]],
  cocky: () => [...logPart(), [JOEY, 13, 13], [COCKY, 19, 0, 'f1'], [COCKY.slice(7), 19, 7, 'f2'], [COCKY_OPEN, 19, 0, 'f2'], [SCREECH, 30, 1, 'f2']],
};
export const MOMENTS = {
  // merged: the kangaroo clears the station fence in one bound; a kookaburra laughs on the post (hop)
  clear: () => [[['.....qqqqqqqqqqqqqqq......'], 4, 24], [['oo', 'oo', 'oO', 'oO', 'oO', 'oO', 'oO'], 3, 17], [['oo', 'oo', 'oO', 'oO', 'oO', 'oO', 'oO'], 28, 17],
    [['iiiiiiiiiiiiiiiiiiiiiiiiiiii'], 3, 18], [['iiiiiiiiiiiiiiiiiiiiiiiiiiii'], 3, 21], [ROO_AIR, 5, 0],
    [['.FFF..', 'FFkuXX', 'FFFFZZ', 'UUFF..', 'UVUF..', 'JUU...', 'J.Y...'], 0, 10, 'f1'], [['....X.', '.FFFX.', 'FFkuF.', 'FFFFZ.', 'UUFF.Z', 'UVUF..', 'JUU...', 'J.Y...'], 0, 9, 'f2']],
  // review FAIL: a magpie swoops the kangaroo, which ducks (shake)
  swoop: () => [[RSHADOW, 3, 24], [ROO_CROUCH.map((r, i) => i < 3 ? '.'.repeat(r.length) : r), 4, 5], [MAGPIE, 9, 0, 'f1'], [MAGPIE_DOWN, 13, 2, 'f2'],
    [['Q.Q', '.Q.', 'Q.Q'], 29, 2, 'f2']],
  // needs you: a cockatoo screeches on the hollow log (pulse)
  screech: RAIL.cocky,
};
export const SAYS = { clear: 'over the fence!', swoop: 'swooped!', screech: 'screech!' };

// ---- #241 `row_sprites` (16 × 16) ----
const RR = [
  '...........r.r..',
  '...........RrRr.',
  '..........RRRRR.',
  '..........RkRRRk',
  '.........TTRRR..',
  '.....RRRRTTc....',
  'rr.RRRRRRRccR...',
  '.rrRRRRRRRcc.k..',
  '...rRRRRRRc.....',
  '....rrRRRr......',
  '...rrrrrr.......',
  '.rrrrk..........',
];
const RR_CROUCH = [
  '...........r.r..',
  '...........RrRr.',
  '..........RRRRR.',
  '..........RkRRRk',
  '.........TTRRR..',
  '........RRTTc...',
  '.......RRRRccRk.',
  '......RRRRRcc...',
  '....rrRRRRRRc...',
  '..rr..rRRRRRR...',
  '.rr...rrrRRRr...',
  'rr.....rrrrrrrk.',
];
export const ROWS = {
  bound: [[RR_CROUCH, 0, 3, 'f1'], [RR, 0, 1, 'f2'], [['.qqqqqqqqqq.'], 2, 15]],
  dig: [[['..wwwwww.......', '.wWWWWWWWw.....', 'wWWWWWWWWWwW...', 'wWWWWWWWWTWkW.e', 'wWWWWWWWWTWWWnD', '.wWWWWWWWWWWnnD', '..ww.ww..ww.DDD'], 1, 8], [['eDDDD'], 11, 15],
    [['d.', '.e'], 0, 5, 'f1'], [['e', '.', 'd'], 2, 3, 'f2']],
  peer: [[['pppPPPPPPPPPPPP'], 0, 11], [['pppPPPPPPPPPPPP'], 0, 12], [Array(16).fill('PPPp'), 12, 0], [['.l', 'lL'], 0, 13],
    ...f([[['EE......EE', 'EGGGGGGGGE', 'GGGGGGGGGG', 'GGGGGGGGGG', 'GkGGGGGGkG', 'GGGGNNGGGG', '.GGGNNGGG.', '..TTTTTT..'], 1, 2], [['gg....gg'], 2, 10]], 'f1'),
    ...f([[['EE......EE', 'EGGGGGGGGE', 'GGGGGGGGGG', 'GGGGGGGGGG', 'GkGGGGGGkG', 'GGGGNNGGGG', '.GGGNNGGG.', '..TTTTTT..'], 0, 3], [['gg....gg'], 2, 10]], 'f2')],
  run: [[['......kMM.', '.....MMMkX', '.....BBB..', '....BTB...', '.mMMMMB...', 'mMMMMMMM..', 'mMmMMmMMm.', '.mMMMMMm..'], 2, 1],
    ...f([...line(6, 9, 4, 13, 'Y'), [['YY'], 2, 14], ...line(7, 9, 9, 13, 'Y'), [['YY'], 9, 14]], 'f1'), ...f([...line(6, 9, 6, 13, 'Y'), [['YY'], 6, 14], ...line(7, 9, 8, 13, 'Y'), [['Y'], 9, 14]], 'f2')],
  sleep: [[['P.......P', 'PP.....PP', '.PP...PP.', '..PPPPP..', '...PPP...'], 3, 11], [['EE....EE', 'EGGGGGGE', 'GkkGGkkG', 'GGGNNGGG', '.GGNNGG.', '..gggg..'], 3, 5],
    [['QQ', '.Q', 'QQ'], 12, 1, 'f1'], [['QQ', '.Q', 'QQ'], 13, 0, 'f2']],
  laugh: [[['...FFFF.......', '..FFuuuF......', '.FFFFkuFXXXX..', '.FFFFFFFZZZZX.', '..FFFFFF......', '.UUFTTFF......', 'UUVUFFFF......', 'UVUUUFF.......', 'JUUUUU........', 'JJ.Y.Y........', '...oOOo.......'], 1, 3, 'f1'],
    [['........XX....', '...FFFFXXX....', '..FFuuuFX.....', '.FFFFkuF......', '.FFFFFFFZZ....', '..FFFFFF.ZZZ..', '.UUFTTFF......', 'UUVUFFFF......', 'UVUUUFF.......', 'JUUUUU........', 'JJ.Y.Y........', '...oOOo.......'], 1, 2, 'f2'],
    [['Q.Q', 'QQQ', 'Q.Q'], 13, 3, 'f2']],
  stuck: [[['....eDDDDDDe....', '..eDDDDDDDDDDe..', '.eDDDAAAAAADDDe.', 'eDDDAAAAAAAADDDd', 'DDDAAAAAAAAAADDd', 'DDDAAAAAAAAAADdd'], 0, 9],
    [['..wwwwww..', '.wWWWWWWw.', 'wWWWWwWWWw', 'wWWWWwWWWw', 'wwWWWWWWww'], 3, 10], [['w'], 7, 9], [['ww'], 2, 13, 'f1'], [['ww'], 12, 12, 'f1'], [['ww'], 2, 12, 'f2'], [['ww'], 12, 13, 'f2']],
  joey: [[['aaaaaaaaaaaaaaaa', ...Array(6).fill('OOOOOOOOOOOOOOOO'), 'oooooooooooooooo'], 0, 7], [['.oooooo.', 'oAAAAAAo', 'oAAAAAAo', 'oAAAAAAo', '.oooooo.'], 4, 8], [['r..r', 'RrRr', 'kRRk'], 6, 6, 'f1'], [['r..r', 'RrRr', 'kRRk'], 6, 7, 'f2']],
  needs: [[['..H.H.', '..HHH.', '.fQQQf', 'fQQkQQii', 'fQQQQQi.', 'fQQQQQf.', '.fQQQf..', '..i.i...'], 5, 0, 'f1'], [['.H..HH', '..HHHH.', '.fQQQf.', 'fQQkQQii', 'fQQQQf..', 'fQQQQii.', '.fQQQf..', '..i.i...'], 5, 0, 'f2'],
    [['aaaaaaaaaaaaaaaa', 'OOOOOOOOOOOOOOOO', 'OOooOOOOOOoOOOOO', 'OOOOOOOOOOOOOOOO', 'oooooooooooooooo'], 0, 8], [['Q', '.', 'Q'], 14, 0, 'f2']],
  merged: [[['o..........o', 'o..........o', 'iiiiiiiiiiii', 'o..........o', 'iiiiiiiiiiii', 'o..........o'], 2, 10], [RR, 0, -1, 'f1'], [RR, 0, -2, 'f2']],
  fail: [[RR_CROUCH.slice(3), 0, 7], [['kk.......', '.kkk...kQ', '..kQQkkkki', '...kkkQ..'], 5, 0, 'f1'], [['kk........', '.kkkQQkkkki', '..QQkkk...'], 6, 2, 'f2']],
};

// ---- rasterising: layers → <rect> runs, one <g> per frame class ----
function rects(map, x0, y0, u) {
  let s = '';
  map.forEach((row, j) => {
    let i = 0;
    while (i < row.length) {
      const c = row[i];
      if (c === '.' || c === ' ' || !INK[c]) { i++; continue; }
      let n = 1;
      while (row[i + n] === c) n++;
      const [fill, op] = Array.isArray(INK[c]) ? INK[c] : [INK[c], 1];
      s += `<rect x="${(x0 + i) * u}" y="${(y0 + j) * u}" width="${n * u}" height="${u}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
      i += n;
    }
  });
  return s;
}
export function draw(layers, u = 4) {
  return layers.map(([map, x, y, cls]) => cls ? `<g class="${cls}">${rects(map, x, y, u)}</g>` : rects(map, x, y, u)).join('');
}
// Symbols for a page: a-<action>, m-<moment>, q-<rail item> (132 × 100), r-<row> (16 × 16).
export function symbols() {
  const sym = (id, vb, body) => `<symbol id="${id}" viewBox="${vb}" shape-rendering="crispEdges">${body}</symbol>`;
  let s = '';
  for (const [k, fn] of Object.entries(ACTIONS)) s += sym(`a-${k}`, '0 0 132 100', draw(fn()));
  for (const [k, fn] of Object.entries(MOMENTS)) s += sym(`m-${k}`, '0 0 132 100', draw(fn()));
  for (const [k, fn] of Object.entries(RAIL)) s += sym(`q-${k}`, '0 0 132 100', draw(fn()));
  for (const [k, m] of Object.entries(ROWS)) s += sym(`r-${k}`, '0 0 16 16', draw(m, 1));
  s += sym('loading', '0 0 132 100', draw(ACTIONS.bound()));
  return s;
}

// ---- the per-lane looks: one set for all four animals, 8 of them like the farm's coats (the same stable pick) ----
// A look is a kangaroo coat (red, grey, sandy), a wombat coat, a koala coat and an emu coat.
const ROO = { red: ['#b8643a', '#8c4527', '#ecd2aa'], grey: ['#9b8e82', '#76695e', '#e3dacb'], sandy: ['#c9935a', '#a0703f', '#f0dcb8'], dusk: ['#a3573f', '#7d3e2c', '#e8c7a6'] };
const WOM = { brown: ['#7d6a58', '#5b4b3e'], grey: ['#80786f', '#5f5852'], sandy: ['#9b8166', '#78624c'], dark: ['#5e5047', '#433832'] };
const KOA = { grey: ['#9ea3a8', '#71777d'], silver: ['#b3b6b6', '#878b8c'], brown: ['#9a8f86', '#71675f'] };
const EMUC = { brown: ['#5c4a38', '#3f3226'], dark: ['#3f362e', '#2a231d'], tawny: ['#7a6248', '#584532'] };
export const LOOKS = [
  ['red', 'brown', 'grey', 'brown'], ['grey', 'grey', 'silver', 'dark'], ['sandy', 'sandy', 'brown', 'tawny'], ['dusk', 'dark', 'grey', 'brown'],
  ['grey', 'brown', 'brown', 'tawny'], ['red', 'grey', 'silver', 'dark'], ['sandy', 'dark', 'grey', 'brown'], ['dusk', 'sandy', 'silver', 'tawny'],
].map(([ro, wo, ko, em]) => ({
  ro, wo, ko, em,
  vars: { '--ro': ROO[ro][0], '--rs': ROO[ro][1], '--rb': ROO[ro][2], '--wo': WOM[wo][0], '--ws': WOM[wo][1], '--ko': KOA[ko][0], '--ks': KOA[ko][1], '--em': EMUC[em][0], '--es': EMUC[em][1] },
}));
export const FLEET = { l: '#58a6ff', p: '#bc8cff', c: '#39c5cf', g: '#3fb950', a: '#d29922' };
export const style = (look, fk = 'l') => Object.entries({ ...LOOKS[look].vars, '--fk': FLEET[fk] || fk }).map(([k, v]) => `${k}:${v}`).join(';');

// ---- the Fire Danger Rating sign: four bands, Moderate · High · Extreme · Catastrophic (the AFDRS colours) ----
export const LEVELS = ['moderate', 'high', 'catastrophic'];
const BANDS = ['#3e9a4a', '#f2c31a', '#e8731e', '#c41f1e'];
const NEEDLE = { moderate: 157.5, high: 112.5, catastrophic: 22.5 };
// The dial as grid cells: a half ring centred on (cx, cy), radius r0..r1, each cell coloured by its angle.
function dial(cx, cy, r0, r1, u, x0 = 0, y0 = 0) {
  let s = '';
  for (let y = Math.floor(cy - r1); y < cy; y++) for (let x = Math.floor(cx - r1); x < cx + r1; x++) {
    const dx = x + 0.5 - cx, dy = cy - (y + 0.5), d = Math.hypot(dx, dy);
    if (d < r0 || d > r1) continue;
    const ang = Math.atan2(dy, dx) * 180 / Math.PI, b = Math.min(3, Math.floor((180 - ang) / 45));
    s += `<rect x="${(x0 + x) * u}" y="${(y0 + y) * u}" width="${u}" height="${u}" fill="${BANDS[b]}"/>`;
  }
  return s;
}
function needle(cx, cy, len, deg, u, x0 = 0, y0 = 0, w = 1) {
  const a = deg * Math.PI / 180, cells = new Set();
  for (let t = 0; t <= len; t += 0.35) cells.add(`${Math.floor(cx + Math.cos(a) * t)},${Math.floor(cy - Math.sin(a) * t)}`);
  let s = '';
  for (const c of cells) { const [x, y] = c.split(',').map(Number); s += `<rect x="${(x0 + x) * u}" y="${(y0 + y) * u}" width="${u * w}" height="${u}" fill="#1b1b1b"/>`; }
  return s;
}
const LABEL = { moderate: 'MODERATE', high: 'HIGH', catastrophic: 'CATASTROPHIC' };
// The roadside sign, in scene units (u = 4): the board, the dial, the needle per level (`.s-<level>`, a 4° wobble on f2).
function sign(x, y, u) {
  const r = (a, b, w, h, fill) => `<rect x="${(x + a) * u}" y="${(y + b) * u}" width="${w * u}" height="${h * u}" fill="${fill}"/>`;
  let s = r(5, 18, 2, 8, '#6b6f73') + r(23, 18, 2, 8, '#6b6f73') + r(5, 18, 1, 8, '#8d9196') + r(23, 18, 1, 8, '#8d9196');
  s += r(-0.5, -0.5, 31, 20, '#1d4a2c') + r(0, 0, 30, 19, '#f4efe2') + r(0, 0, 30, 4, '#1d4a2c');
  s += `<text x="${(x + 15) * u}" y="${(y + 2.9) * u}" font-size="${2.6 * u}" font-weight="800" font-family="Helvetica,Arial,sans-serif" text-anchor="middle" fill="#f4efe2" letter-spacing=".3">FIRE DANGER RATING</text>`;
  s += dial(15, 15, 6, 10.6, u, x, y) + r(13.5, 13.5, 3, 1.5, '#1b1b1b') + r(3, 15, 24, 0.5, '#1d4a2c');
  for (const l of LEVELS) {
    s += `<g class="s-${l}"><g class="f1">${needle(15, 15, 9, NEEDLE[l], u, x, y)}</g><g class="f2">${needle(15, 15, 9, NEEDLE[l] - 5, u, x, y)}</g>` +
      `<text x="${(x + 15) * u}" y="${(y + 18.4) * u}" font-size="${2.6 * u}" font-weight="800" font-family="Helvetica,Arial,sans-serif" text-anchor="middle" fill="${BANDS[{ moderate: 0, high: 1, catastrophic: 3 }[l]] === '#f2c31a' ? '#8a6a00' : BANDS[{ moderate: 0, high: 1, catastrophic: 3 }[l]]}">${LABEL[l]}</text></g>`;
  }
  return s;
}

// ---- the clearing (scene.svg): one repo's clearing of ghost gums on red earth along a dirt track, 392 × 188, one pixel
// = 4. A billabong at the front, the station gate over the track in the repo colour (`--fk`), the roadside sign on the
// right. `.sky-day` / `.sky-night` follow the scheme's light/dark; `.s-<level>` and `.s-up` (high and catastrophic) the
// load: heat shimmer over the track and cicadas rising from the gums. ----
const STARS = [[6, 3], [14, 8], [22, 2], [31, 6], [38, 11], [47, 4], [52, 9], [86, 3], [93, 9], [80, 12], [44, 14], [27, 12], [9, 13], [60, 2]];
const CROSS = [[70, 3, 3], [71, 15, 3], [65, 9, 3], [75.5, 8, 2], [73.5, 12, 1]];
function tree(x, y, h, k, u) {
  const r = (a, b, w, hh, fill) => `<rect x="${(x + a) * u}" y="${(y + b) * u}" width="${w * u}" height="${hh * u}" fill="${fill}"/>`;
  let s = r(0, 0, 2, h, '#e6dfcf') + r(1.5, 0, 0.5, h, '#b6ab94');
  const lim = k ? [[-1, -1, -4, -6], [2, -2, 5, -7], [0, 0, -3, -4]] : [[-1, -1, -3, -5], [2, -1, 4, -6]];
  for (const [a, b, c, d] of lim) for (let t = 0; t <= 1; t += 0.12) s += r(a + c * t, b + d * t, 1, 1, '#e6dfcf');
  const can = k ? [[-9, -11, 9, 4], [-6, -13, 7, 3], [1, -12, 10, 4], [3, -14, 6, 3], [-4, -9, 9, 3]] : [[-7, -9, 7, 3], [1, -10, 7, 3], [-3, -11, 6, 3]];
  for (const [a, b, w, hh] of can) s += r(a, b, w, hh, '#7d9a66') + r(a, b + hh - 1, w, 1, '#5b7a4c') + r(a + 1, b, w - 3, 0.75, '#9bb583');
  return s;
}
export function clearing(u = 4) {
  const W = 98, H = 47, r = (x, y, w, h, fill, extra = '') => `<rect x="${x * u}" y="${y * u}" width="${w * u}" height="${h * u}" fill="${fill}"${extra}/>`;
  const grad = (id, a, b) => `<linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient>`;
  let s = `<defs>${grad('bsd', '#b9d8e3', '#f1e2c0')}${grad('bsn', '#0e1035', '#2c285a')}</defs>`;
  s += `<g class="sky-day">${r(0, 0, W, 27, 'url(#bsd)')}${r(8, 3, 5, 5, '#fff3c2')}${r(7, 4, 7, 3, '#fff3c2')}${r(9, 4, 3, 3, '#fffbe6')}</g>`;
  s += `<g class="sky-night">${r(0, 0, W, 27, 'url(#bsn)')}`;
  for (const [x, y] of STARS) s += r(x, y, 0.5, 0.5, '#e8e6f5');
  for (const [x, y, k] of CROSS) s += k > 1 ? r(x, y - (k - 1) / 2, 0.75, k - 0.5, '#ffffff') + r(x - (k - 1) / 2 + 0.125, y, k - 0.5, 0.75, '#ffffff') : r(x, y, 0.75, 0.75, '#ffffff');
  s += r(56, 12, 1, 1, '#fff6d8') + r(60, 10.5, 1, 1, '#fff6d8') + '</g>';
  // the far range: a flat-topped mesa and low hills, then the red earth
  s += `<g class="sky-day">${r(52, 19, 26, 8, '#c98a62')}${r(55, 17, 19, 2, '#c98a62')}${r(57, 16, 14, 1, '#d29a72')}${r(0, 23, 30, 4, '#b98a6a')}${r(78, 22, 20, 5, '#b98a6a')}</g>`;
  s += `<g class="sky-night">${r(52, 19, 26, 8, '#3a2f55')}${r(55, 17, 19, 2, '#3a2f55')}${r(57, 16, 14, 1, '#463a64')}${r(0, 23, 30, 4, '#2e2848')}${r(78, 22, 20, 5, '#2e2848')}</g>`;
  s += r(0, 26, W, H - 26, '#b5562e') + r(0, 26, W, 2, '#c4693c');
  for (let i = 0; i < 40; i++) s += r((i * 37) % W, 29 + ((i * 11) % 17), 1, 0.5, i % 2 ? '#a24b27' : '#c9703f');
  // the dirt track: from the front to the horizon, pale dust with two ruts
  for (let y = 26; y < H; y++) { const t = (y - 26) / (H - 26), c = 46 + t * 6, hw = 1.5 + t * 13; s += r(c - hw, y, hw * 2, 1, '#d9a070') + r(c - hw * 0.45, y, 0.75, 1, '#c48558') + r(c + hw * 0.45 - 0.75, y, 0.75, 1, '#c48558'); }
  // spinifex tufts
  for (const [x, y] of [[4, 30], [20, 28], [33, 33], [64, 30], [70, 41], [90, 30], [84, 44], [12, 35]]) s += r(x, y, 3, 1, '#b8a84a') + r(x + 0.5, y - 1, 0.5, 1, '#c9bb5a') + r(x + 2, y - 1, 0.5, 1, '#c9bb5a') + r(x + 1, y - 1.5, 0.5, 1.5, '#d6c86a');
  // ghost gums
  s += tree(11, 21, 12, 1, u) + tree(32, 18, 10, 0, u) + tree(93, 20, 9, 0, u);
  // the station gate over the track, its crossbar and name board in the repo colour
  s += r(38, 17, 1.25, 12, '#62482f') + r(57, 17, 1.25, 12, '#62482f') + r(37.5, 17, 21, 1, '#8b6a48') + r(42, 15, 12, 3, 'var(--fk)') + r(42, 17.5, 12, 0.5, '#241a14', ' opacity=".3"');
  s += r(43, 16, 10, 0.75, '#ffffff', ' opacity=".55"');
  // the billabong at the front: still water, reeds, a ripple
  s += r(4, 40, 26, 7, '#5d8ea3') + r(2, 41, 30, 6, '#5d8ea3') + r(6, 39, 20, 1, '#5d8ea3') + r(8, 42, 9, 0.5, '#9cc4cf') + r(18, 44, 7, 0.5, '#9cc4cf');
  for (const x of [3, 4.5, 29, 30.5]) s += r(x, 37.5, 0.5, 3, '#6d8a4a') + r(x - 0.5, 36.5, 1.5, 1.5, '#4f3a2a');
  // the hollow log lies by the track (the rail's log, in the scene)
  s += r(62, 36, 16, 3, '#8b6a48') + r(62, 36, 16, 0.75, '#ad8a62') + r(62, 39, 16, 0.75, '#62482f') + r(61, 36, 2, 3.5, '#62482f') + r(61.5, 36.5, 1.25, 2.5, '#2f2117');
  // the sign
  s += `<g class="fdr">${sign(66, 14, u)}</g>`;
  // the night over the ground, and the load: heat shimmer (high, catastrophic) and cicadas rising
  s += `<g class="sky-night">${r(0, 26, W, H - 26, '#0c0f33', ' opacity=".5"')}${r(11, 42, 1, 0.5, '#e8e6f5')}${r(22, 43, 1, 0.5, '#e8e6f5')}</g>`;
  const shimmer = k => { let t = ''; for (let i = 0; i < 9; i++) { const y = 25 + (i % 3) * 1.5, x = (i * 11 + k * 3) % 90; t += r(x, y, 6, 0.5, '#fff4d8', ' opacity=".45"') + r(x + 6, y - 0.5, 4, 0.5, '#fff4d8', ' opacity=".35"'); } return t; };
  s += `<g class="s-up"><g class="f1">${shimmer(0)}</g><g class="f2">${shimmer(1)}</g></g>`;
  const cic = (n, k) => { let t = ''; for (let i = 0; i < n; i++) { const x = 4 + ((i * 13 + k * 5) % 40), y = 3 + ((i * 7 + k * 3) % 13); t += r(x, y, 1, 0.75, '#2e3a24') + r(x - 0.5, y - 0.25, 0.5, 0.5, '#e6efe0', ' opacity=".7"') + r(x + 1, y - 0.25, 0.5, 0.5, '#e6efe0', ' opacity=".7"'); } return t; };
  s += `<g class="s-high"><g class="f1">${cic(4, 0)}</g><g class="f2">${cic(4, 1)}</g></g><g class="s-catastrophic"><g class="f1">${cic(11, 0)}</g><g class="f2">${cic(11, 1)}</g>${r(0, 0, W, 27, '#f0a050', ' opacity=".16"')}</g>`;
  return `<svg viewBox="0 0 ${W * u} ${H * u}" preserveAspectRatio="xMidYMax slice" shape-rendering="crispEdges" aria-hidden="true">${s}</svg>`;
}

// The sign in miniature for the Host card header (#370 `minis`, 48 × 16): the dial and its needle, no words.
export function mini(level, frame = 0) {
  let s = '<rect width="48" height="16" rx="1" fill="#1d4a2c"/><rect x="1" y="1" width="46" height="14" fill="#f4efe2"/>';
  s += dial(24, 15, 5, 13.4, 1) + needle(24, 15, 12, NEEDLE[level] - frame * 5, 1, 0, 0) + '<rect x="22" y="13" width="4" height="2" fill="#1b1b1b"/>';
  return `<svg class="hm" viewBox="0 0 48 16" shape-rendering="crispEdges" aria-hidden="true">${s}</svg>`;
}

// ---- #241 `icons`: a yellow roo-crossing sign (black kangaroo on a yellow diamond) on a post, sky over red earth ----
const ROO_SIGN = [
  '......................k..k....',
  '......................kk.kk...',
  '.......................kkkk...',
  '......................kkkkkk..',
  '......................kkkkkkkk',
  '.....................kkkkkkkkk',
  '....................kkkkk.....',
  '..................kkkkkk......',
  '...............kkkkkkkk.......',
  '............kkkkkkkkkkk.k.....',
  '..........kkkkkkkkkkkkk..k....',
  '.........kkkkkkkkkkkkkk...k...',
  '........kkkkkkkkkkkkkk........',
  '.......kkkkkkkkkkkkkk.........',
  '......kkkkkkkkkkkkkkk.........',
  '.....kkk.kkkkkkkkkkk..........',
  '....kkk...kkkkkkkkkk..........',
  '...kkk.....kkkkkkkkk..........',
  '..kkk.......kkkkkkkk..........',
  '.kkk.........kkkkkk...........',
  'kkk...........kkkkkkk.........',
  'kk.............kkkkkkkkkk.....',
  '.................kkkkkkkkkkk..',
];
function diamond(N, cx, cy, R, roo, rx, ry, bw = 1.6) {
  const g = Array.from({ length: N }, () => Array(N).fill('.'));
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    const d = Math.abs(x + 0.5 - cx) + Math.abs(y + 0.5 - cy);
    if (d <= R) g[y][x] = d > R - bw ? 'k' : 'Y';
  }
  roo.forEach((row, j) => [...row].forEach((c, i) => { if (c === 'k' && g[ry + j]?.[rx + i] === 'Y') g[ry + j][rx + i] = 'k'; }));
  return g.map(r => r.join(''));
}
const SIGNINK = { Y: '#f6c512', H: '#f6c512', k: '#111111' };
const px = (map, u, ink) => map.map((row, j) => [...row].map((c, i) => ink[c] ? `<rect x="${i * u}" y="${j * u}" width="${u}" height="${u}" fill="${ink[c]}"/>` : '').join('')).join('');
export function icon(size = 512) {
  const d = diamond(64, 32, 30, 29, ROO_SIGN, 17, 19, 3);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 512 512" shape-rendering="crispEdges">` +
    `<defs><linearGradient id="isky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#9fcde0"/><stop offset="1" stop-color="#f1e2c0"/></linearGradient></defs>` +
    `<rect width="512" height="512" fill="url(#isky)"/><rect y="432" width="512" height="80" fill="#b5562e"/><rect y="432" width="512" height="16" fill="#c9703f"/>` +
    `<rect x="240" y="472" width="32" height="40" fill="#6b6f73"/><rect x="240" y="472" width="10" height="40" fill="#8d9196"/>${px(d, 8, SIGNINK)}</svg>`;
}
// The favicon (16 px): the diamond alone, a simpler roo, readable at 16.
const ROO_FAV = ['......k.k', '......kkk', '....kkkk.', '..kkkkkk.', 'kkkkkkk..', 'k..kkk...', '....kkkk.'];
export function favicon(size = 32) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 16 16" shape-rendering="crispEdges"><rect width="16" height="13" fill="#9fcde0"/><rect y="13" width="16" height="3" fill="#b5562e"/>${px(diamond(16, 8, 8, 8, ROO_FAV, 3, 5), 1, SIGNINK)}</svg>`;
}

// ---- ascii (#335): the Lanes cells (≤ 3 lines of 8, the farm's), the row figures (3 characters), the Host load (≤ 5),
// the loading frames. The roo, the wombat, the koala, the emu, the kookaburra, by role and state. ----
export const ASCII = {
  impl: '   //_\n~__( o>\n //  \\\\', fix: "'. ,\n(___)o^\n '' ''", rev: '=====||\n (oYo)||\n      ||',
  update: '    _o>\n (###)/\n  / >', idle: ' \\ z /\n (-Y-)\n   \\|/', done: 'ha! ha!\n  (o<=\n  |##|', blocked: "  _..._\n / () \\\n/_/''\\_\\",
};
export const ASCII_ROWS = { impl: '~R^', fix: '.W:', rev: '=K|', update: 'E>>', idle: 'zK.', done: 'ha!', blocked: '[W]' };
export const ASCII_LOAD = { calm: '(\\  )', busy: '( | )', held: '(  /)' };
export const ASCII_LOADING = '~R^\n_R_';
