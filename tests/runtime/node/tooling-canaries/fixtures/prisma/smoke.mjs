import { PrismaClient } from "@prisma/client";

// Prisma 7's generated "client" engine now validates the constructor shape
// before it reaches query execution. Using an explicit empty options object
// yields the stable adapter/accelerate requirement instead of the vaguer
// "missing options" initialization path.
const prisma = new PrismaClient({});

try {
  const created = await prisma.user.create({
    data: {
      email: "ada@example.com",
      name: "Ada",
    },
  });
  const count = await prisma.user.count();
  const found = await prisma.user.findUnique({
    where: {
      email: "ada@example.com",
    },
  });
  console.log(
    JSON.stringify({
      createdEmail: created.email,
      count,
      foundName: found?.name ?? null,
    }),
  );
} finally {
  await prisma.$disconnect();
}
