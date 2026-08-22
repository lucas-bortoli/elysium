function distance(a, b) {
    const dx = a.x - b.x;
    const dy = a.y - b.y;
    return Math.sqrt(dx * dx + dy * dy);
}
class Vector {
    x;
    y;
    constructor(x, y) {
        this.x = x;
        this.y = y;
    }
    add(other) {
        return new Vector(this.x + other.x, this.y + other.y);
    }
}
const origin = { x: 0, y: 0 };
const v = new Vector(3, 4);
print(`distance from origin: ${distance(origin, v)}`);
print(`v + origin = (${v.add(origin).x}, ${v.add(origin).y})`);
function divide(a, b) {
    if (b === 0) {
        throw new Error("divide: cannot divide by zero");
    }
    return a / b;
}
// Intentional error to exercise stack trace reporting.
print(`10 / 0 = ${divide(10, 0)}`);
const bad = "oops";
