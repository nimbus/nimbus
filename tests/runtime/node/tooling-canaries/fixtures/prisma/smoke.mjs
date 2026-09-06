import { PrismaClient } from "@prisma/client";
import { PrismaLibSql } from "@prisma/adapter-libsql";

const adapter = new PrismaLibSql({
  url: process.env.DATABASE_URL,
});
const prisma = new PrismaClient({ adapter });

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
