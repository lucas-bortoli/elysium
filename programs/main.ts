interface Point {
    x: number;
    y: number;
}

function distance(a: Point, b: Point): number {
    const dx: number = a.x - b.x;
    const dy: number = a.y - b.y;
    return Math.sqrt(dx * dx + dy * dy) as number;
}

class Vector implements Point {
    x: number;
    y: number;

    constructor(x: number, y: number) {
        this.x = x;
        this.y = y;
    }

    add(other: Point): Vector {
        return new Vector(this.x + other.x, this.y + other.y);
    }
}

const origin: Point = { x: 0, y: 0 };
const v = new Vector(3, 4);

print(`distance from origin: ${distance(origin, v)}`);
print(`v + origin = (${v.add(origin).x}, ${v.add(origin).y})`);

function divide(a: number, b: number): number {
    if (b === 0) {
        throw new Error("divide: cannot divide by zero");
    }
    return a / b;
}

// Intentional error to exercise stack trace reporting.
print(`10 / 0 = ${divide(10, 0)}`);
