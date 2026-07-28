import sqlite3

db_path = r'C:\Users\Ayin\AppData\Roaming\com.ayinaki.ayinlauncher\app.db'
conn = sqlite3.connect(db_path)
c = conn.cursor()
c.execute("SELECT id, name, icon_path FROM instances;")
for row in c.fetchall():
    print(f"ID: {row[0]} | Name: {row[1]} | Icon: {row[2]}")
